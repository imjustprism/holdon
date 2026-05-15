use std::time::Instant;

use mongodb::Client;
use mongodb::bson::doc;
use mongodb::options::ClientOptions;
use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, err_stage, install_rustls_provider_once, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

impl Hintable for mongodb::error::Error {
    fn hint(&self) -> Option<&'static str> {
        use mongodb::error::ErrorKind;
        match self.kind.as_ref() {
            ErrorKind::Authentication { .. } => Some(hints::MONGODB_AUTH),
            ErrorKind::InvalidTlsConfig { .. } => Some(hints::MONGODB_TLS),
            ErrorKind::ServerSelection { message, .. } => {
                let lower = message.to_ascii_lowercase();
                if lower.contains("no primary")
                    || lower.contains("replicasetnoprimary")
                    || lower.contains("replica set")
                {
                    Some(hints::MONGODB_NO_PRIMARY)
                } else {
                    Some(hints::MONGODB_NOT_READY)
                }
            }
            _ => Some(hints::MONGODB_NOT_READY),
        }
    }
}

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    install_rustls_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let conn_str = url.as_str().to_owned();
    let stage = match ping(&conn_str, ctx).await {
        Ok(()) => ok_stage(StageKind::Mongodb, start.elapsed()),
        Err(e) => {
            let hint = e.hint();
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &conn_str);
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Mongodb, start.elapsed(), msg, hint)
        }
    };
    vec![stage]
}

async fn ping(uri: &str, ctx: AttemptCtx) -> mongodb::error::Result<()> {
    let mut opts = ClientOptions::parse(uri).await?;
    opts.connect_timeout = Some(ctx.attempt_timeout);
    opts.server_selection_timeout = Some(ctx.attempt_timeout);
    let client = Client::with_options(opts)?;
    client
        .database("admin")
        .run_command(doc! { "ping": 1 })
        .await?;
    Ok(())
}
