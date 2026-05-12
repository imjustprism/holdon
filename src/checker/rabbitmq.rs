use std::sync::OnceLock;
use std::time::Instant;

use lapin::options::{ExchangeDeclareOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties, ExchangeKind};
use tokio::time::timeout;
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

pub(super) async fn probe(
    url: &Url,
    queue: Option<&str>,
    exchange: Option<&str>,
    ctx: AttemptCtx,
) -> Vec<Stage> {
    install_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let conn_str = strip_query(url);
    let stage = match timeout(ctx.attempt_timeout, run(&conn_str, queue, exchange)).await {
        Ok(Ok(())) => ok_stage(StageKind::Rabbitmq, start.elapsed()),
        Ok(Err(e)) => {
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &conn_str);
                msg = redact_in(&msg, &pw);
            }
            let hint = hint_for(&msg);
            err_stage(StageKind::Rabbitmq, start.elapsed(), msg, Some(hint))
        }
        Err(_) => err_stage(
            StageKind::Rabbitmq,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::RABBITMQ_NOT_READY),
        ),
    };
    vec![stage]
}

async fn run(uri: &str, queue: Option<&str>, exchange: Option<&str>) -> lapin::Result<()> {
    let conn = Connection::connect(uri, ConnectionProperties::default()).await?;
    let result = declare_checks(&conn, queue, exchange).await;
    let _ = conn.close(0, "ok").await;
    result
}

async fn declare_checks(
    conn: &Connection,
    queue: Option<&str>,
    exchange: Option<&str>,
) -> lapin::Result<()> {
    if queue.is_none() && exchange.is_none() {
        return Ok(());
    }
    let channel = conn.create_channel().await?;
    if let Some(q) = queue {
        channel
            .queue_declare(
                q,
                QueueDeclareOptions {
                    passive: true,
                    ..QueueDeclareOptions::default()
                },
                FieldTable::default(),
            )
            .await?;
    }
    if let Some(x) = exchange {
        channel
            .exchange_declare(
                x,
                ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    passive: true,
                    ..ExchangeDeclareOptions::default()
                },
                FieldTable::default(),
            )
            .await?;
    }
    Ok(())
}

fn strip_query(url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.into()
}

fn install_provider_once() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn hint_for(msg: &str) -> &'static str {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("access_refused") {
        if lower.contains("vhost") {
            hints::RABBITMQ_VHOST
        } else {
            hints::RABBITMQ_AUTH
        }
    } else if lower.contains("not_allowed") {
        hints::RABBITMQ_VHOST
    } else if lower.contains("not_found")
        || lower.contains("no queue")
        || lower.contains("no exchange")
    {
        hints::RABBITMQ_QUEUE
    } else if lower.contains("tls") || lower.contains("certificate") {
        hints::RABBITMQ_TLS
    } else {
        hints::RABBITMQ_NOT_READY
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hint_for_classifies_auth() {
        assert_eq!(
            hint_for("ACCESS_REFUSED - Login failed"),
            hints::RABBITMQ_AUTH
        );
    }

    #[test]
    fn hint_for_classifies_vhost() {
        assert_eq!(
            hint_for("ACCESS_REFUSED - vhost 'foo' not allowed"),
            hints::RABBITMQ_VHOST
        );
        assert_eq!(
            hint_for("NOT_ALLOWED - access to vhost refused"),
            hints::RABBITMQ_VHOST
        );
    }

    #[test]
    fn hint_for_classifies_queue() {
        assert_eq!(
            hint_for("NOT_FOUND - no queue 'jobs'"),
            hints::RABBITMQ_QUEUE
        );
        assert_eq!(
            hint_for("no exchange 'events' in vhost '/'"),
            hints::RABBITMQ_QUEUE
        );
    }

    #[test]
    fn hint_for_classifies_tls() {
        assert_eq!(hint_for("certificate verify failed"), hints::RABBITMQ_TLS);
    }

    #[test]
    fn hint_for_falls_back_to_not_ready() {
        assert_eq!(hint_for("connection refused"), hints::RABBITMQ_NOT_READY);
    }
}
