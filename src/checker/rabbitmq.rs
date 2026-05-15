use std::time::Instant;

use lapin::options::{ExchangeDeclareOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties, ExchangeKind};
use tokio::time::timeout;
use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, err_stage, install_rustls_provider_once, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

mod amqp_codes {
    pub(super) const ACCESS_REFUSED: u16 = 403;
    pub(super) const NOT_FOUND: u16 = 404;
    pub(super) const NOT_ALLOWED: u16 = 530;
}

impl Hintable for lapin::Error {
    fn hint(&self) -> Option<&'static str> {
        use amqp_codes::{ACCESS_REFUSED, NOT_ALLOWED, NOT_FOUND};
        use lapin::ErrorKind;
        match self.kind() {
            ErrorKind::ProtocolError(amqp) => match amqp.get_id() {
                ACCESS_REFUSED => {
                    let msg = amqp.get_message().as_str().to_ascii_lowercase();
                    if msg.contains("vhost") {
                        Some(hints::RABBITMQ_VHOST)
                    } else {
                        Some(hints::RABBITMQ_AUTH)
                    }
                }
                NOT_ALLOWED => Some(hints::RABBITMQ_VHOST),
                NOT_FOUND => Some(hints::RABBITMQ_QUEUE),
                _ => Some(hints::RABBITMQ_NOT_READY),
            },
            ErrorKind::IOError(io_err) => match io_err.kind() {
                std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe => Some(hints::RABBITMQ_NOT_READY),
                _ => Some(hints::RABBITMQ_TLS),
            },
            _ => Some(hints::RABBITMQ_NOT_READY),
        }
    }
}

pub(super) async fn probe(
    url: &Url,
    queue: Option<&str>,
    exchange: Option<&str>,
    ctx: AttemptCtx,
) -> Vec<Stage> {
    install_rustls_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let conn_str = strip_query(url);
    let stage = match timeout(ctx.attempt_timeout, run(&conn_str, queue, exchange)).await {
        Ok(Ok(())) => ok_stage(StageKind::Rabbitmq, start.elapsed()),
        Ok(Err(e)) => {
            let hint = e.hint();
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &conn_str);
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Rabbitmq, start.elapsed(), msg, hint)
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
    let _ = conn.close(200, "ok").await;
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use lapin::ErrorKind;
    use lapin::protocol::{AMQPError, AMQPErrorKind, AMQPHardError, AMQPSoftError};

    fn protocol_error(kind: AMQPErrorKind) -> lapin::Error {
        protocol_error_with(kind, "test")
    }

    fn protocol_error_with(kind: AMQPErrorKind, message: &str) -> lapin::Error {
        let err = AMQPError::new(kind, message.into());
        ErrorKind::ProtocolError(err).into()
    }

    #[test]
    fn protocol_access_refused_maps_to_auth() {
        let e = protocol_error(AMQPErrorKind::Soft(AMQPSoftError::ACCESSREFUSED));
        assert_eq!(e.hint(), Some(hints::RABBITMQ_AUTH));
    }

    #[test]
    fn protocol_access_refused_with_vhost_message_maps_to_vhost() {
        let e = protocol_error_with(
            AMQPErrorKind::Soft(AMQPSoftError::ACCESSREFUSED),
            "access to vhost 'foo' refused",
        );
        assert_eq!(e.hint(), Some(hints::RABBITMQ_VHOST));
    }

    #[test]
    fn protocol_not_allowed_maps_to_vhost() {
        let e = protocol_error(AMQPErrorKind::Hard(AMQPHardError::NOTALLOWED));
        assert_eq!(e.hint(), Some(hints::RABBITMQ_VHOST));
    }

    #[test]
    fn protocol_not_found_maps_to_queue() {
        let e = protocol_error(AMQPErrorKind::Soft(AMQPSoftError::NOTFOUND));
        assert_eq!(e.hint(), Some(hints::RABBITMQ_QUEUE));
    }

    #[test]
    fn io_connection_refused_maps_to_not_ready() {
        let io = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let e: lapin::Error = ErrorKind::IOError(std::sync::Arc::new(io)).into();
        assert_eq!(e.hint(), Some(hints::RABBITMQ_NOT_READY));
    }
}
