use std::time::Instant;

use rskafka::client::ClientBuilder;
use tokio::time::timeout;
use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, err_stage, install_rustls_provider_once, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

pub(super) async fn probe(
    url: &Url,
    topic: Option<&str>,
    min_partitions: Option<u32>,
    ctx: AttemptCtx,
) -> Vec<Stage> {
    install_rustls_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let stage = match timeout(ctx.attempt_timeout, run(url, topic, min_partitions)).await {
        Ok(Ok(())) => ok_stage(StageKind::Kafka, start.elapsed()),
        Ok(Err(e)) => {
            let mut msg = e.to_string();
            if !pw.is_empty() {
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Kafka, start.elapsed(), msg, e.hint())
        }
        Err(_) => err_stage(
            StageKind::Kafka,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::KAFKA_NOT_READY),
        ),
    };
    vec![stage]
}

#[derive(Debug)]
enum ProbeError {
    Connect(String),
    Metadata(String),
    TopicMissing(String),
    PartitionShortfall { have: usize, want: u32 },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(m) => write!(f, "connect: {m}"),
            Self::Metadata(m) => write!(f, "metadata: {m}"),
            Self::TopicMissing(t) => write!(f, "topic `{t}` not in broker metadata"),
            Self::PartitionShortfall { have, want } => {
                write!(f, "topic has {have} partitions, need at least {want}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

impl Hintable for ProbeError {
    fn hint(&self) -> Option<&'static str> {
        Some(match self {
            Self::Connect(msg) | Self::Metadata(msg) => {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("tls") || lower.contains("certificate") {
                    hints::KAFKA_TLS
                } else {
                    hints::KAFKA_NOT_READY
                }
            }
            Self::TopicMissing(_) => hints::KAFKA_TOPIC_MISSING,
            Self::PartitionShortfall { .. } => hints::KAFKA_PARTITION_COUNT,
        })
    }
}

async fn run(
    url: &Url,
    topic: Option<&str>,
    min_partitions: Option<u32>,
) -> Result<(), ProbeError> {
    let host = url
        .host_str()
        .ok_or_else(|| ProbeError::Connect("missing host".into()))?;
    let port = url
        .port()
        .ok_or_else(|| ProbeError::Connect("missing port".into()))?;
    let broker = format!("{host}:{port}");
    let mut builder = ClientBuilder::new(vec![broker]);
    if url.scheme().eq_ignore_ascii_case("kafkas") {
        let tls = rustls_client_config();
        builder = builder.tls_config(std::sync::Arc::new(tls));
    }
    let client = builder
        .build()
        .await
        .map_err(|e| ProbeError::Connect(format_error_chain(&e)))?;
    let topics = client
        .list_topics()
        .await
        .map_err(|e| ProbeError::Metadata(format_error_chain(&e)))?;
    if let Some(name) = topic {
        let Some(found) = topics.iter().find(|t| t.name == name) else {
            return Err(ProbeError::TopicMissing(name.to_owned()));
        };
        if let Some(want) = min_partitions {
            let have = found.partitions.len();
            if have < want as usize {
                return Err(ProbeError::PartitionShortfall { have, want });
            }
        }
    }
    Ok(())
}

fn rustls_client_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn probe_error_display_includes_topic_name() {
        let e = ProbeError::TopicMissing("jobs".into());
        assert!(e.to_string().contains("jobs"));
    }

    #[test]
    fn probe_error_hint_routes_tls_messages() {
        let e = ProbeError::Connect("TLS handshake failed".into());
        assert_eq!(e.hint(), Some(hints::KAFKA_TLS));
        let e = ProbeError::Metadata("certificate verify error".into());
        assert_eq!(e.hint(), Some(hints::KAFKA_TLS));
    }

    #[test]
    fn probe_error_hint_routes_topic_to_topic_hint() {
        let e = ProbeError::TopicMissing("orders".into());
        assert_eq!(e.hint(), Some(hints::KAFKA_TOPIC_MISSING));
    }

    #[test]
    fn probe_error_hint_routes_partition_to_partition_hint() {
        let e = ProbeError::PartitionShortfall { have: 1, want: 3 };
        assert_eq!(e.hint(), Some(hints::KAFKA_PARTITION_COUNT));
    }

    #[test]
    fn probe_error_hint_falls_back_to_not_ready_for_plain_connect() {
        let e = ProbeError::Connect("connection refused".into());
        assert_eq!(e.hint(), Some(hints::KAFKA_NOT_READY));
    }
}
