use std::sync::OnceLock;
use std::time::Instant;

use mongodb::Client;
use mongodb::bson::doc;
use mongodb::options::ClientOptions;
use tokio::time::timeout;
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    install_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let conn_str = url.as_str().to_owned();
    let stage = match timeout(ctx.attempt_timeout, ping(&conn_str)).await {
        Ok(Ok(())) => ok_stage(StageKind::Mongodb, start.elapsed()),
        Ok(Err(e)) => {
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &pw);
                msg = redact_in(&msg, &conn_str);
            }
            let hint = hint_for(&msg);
            err_stage(StageKind::Mongodb, start.elapsed(), msg, Some(hint))
        }
        Err(_) => err_stage(
            StageKind::Mongodb,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::MONGODB_NOT_READY),
        ),
    };
    vec![stage]
}

async fn ping(uri: &str) -> mongodb::error::Result<()> {
    let opts = ClientOptions::parse(uri).await?;
    let client = Client::with_options(opts)?;
    client
        .database("admin")
        .run_command(doc! { "ping": 1 })
        .await?;
    Ok(())
}

fn install_provider_once() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn hint_for(msg: &str) -> &'static str {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("authentication") || lower.contains("auth failed") {
        hints::MONGODB_AUTH
    } else if lower.contains("no primary") || lower.contains("replica set") {
        hints::MONGODB_NO_PRIMARY
    } else if lower.contains("tls") || lower.contains("certificate") {
        hints::MONGODB_TLS
    } else {
        hints::MONGODB_NOT_READY
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hint_for_classifies_auth_message() {
        assert_eq!(hint_for("Authentication failed"), hints::MONGODB_AUTH);
        assert_eq!(hint_for("auth failed: bad creds"), hints::MONGODB_AUTH);
    }

    #[test]
    fn hint_for_classifies_no_primary() {
        assert_eq!(hint_for("no primary available"), hints::MONGODB_NO_PRIMARY);
        assert_eq!(
            hint_for("replica set election in progress"),
            hints::MONGODB_NO_PRIMARY
        );
    }

    #[test]
    fn hint_for_routes_generic_selection_to_not_ready() {
        assert_eq!(
            hint_for("Server selection timeout: standalone unreachable"),
            hints::MONGODB_NOT_READY
        );
    }

    #[test]
    fn hint_for_classifies_tls() {
        assert_eq!(hint_for("TLS handshake failed"), hints::MONGODB_TLS);
        assert_eq!(hint_for("certificate verify failed"), hints::MONGODB_TLS);
    }

    #[test]
    fn hint_for_falls_back_to_not_ready() {
        assert_eq!(hint_for("connection refused"), hints::MONGODB_NOT_READY);
    }
}
