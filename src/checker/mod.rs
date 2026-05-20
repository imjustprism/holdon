use std::future::Future;
use std::time::{Duration, Instant};

use tokio::time::timeout;

use crate::diagnostic::{CheckOutcome, Stage, StageKind, StageResult};
use crate::target::Target;
use crate::util::format_error_chain;

mod dns;
#[cfg(feature = "docker")]
mod docker;
mod exec;
mod file;
#[cfg(feature = "grpc")]
mod grpc;
mod hint;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "influxdb")]
mod influxdb;
#[cfg(feature = "k8s")]
mod k8s;
#[cfg(feature = "kafka")]
mod kafka;
mod log;
#[cfg(feature = "mongodb")]
mod mongodb;
#[cfg(feature = "mysql")]
mod mysql;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "rabbitmq")]
mod rabbitmq;
#[cfg(feature = "redis")]
mod redis;
mod tcp;
#[cfg(feature = "temporal")]
mod temporal;
#[cfg(feature = "websocket")]
mod websocket;

pub(crate) use hint::{Hintable, hints};

/// Per-attempt context passed from [`crate::Runner`] down to each checker.
///
/// Carries the wall-clock budget the runner is willing to spend on a single
/// probe attempt. Checkers should honour this value when wiring driver
/// timeouts (connect, server selection, query) so a slow target cannot stall
/// the overall deadline.
///
/// The struct is `#[non_exhaustive]`. Future per-attempt knobs can land
/// without breaking downstream pattern matches.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AttemptCtx {
    pub attempt_timeout: Duration,
}

impl Default for AttemptCtx {
    fn default() -> Self {
        Self {
            attempt_timeout: crate::RunnerConfig::DEFAULT_ATTEMPT_TIMEOUT,
        }
    }
}

impl Target {
    pub async fn probe(&self, ctx: AttemptCtx) -> CheckOutcome {
        let start = Instant::now();
        let stages = match self {
            Self::Tcp { host, port } => tcp::probe(host.as_str(), *port, ctx).await,
            Self::Dns { host } => dns::probe(host.as_str(), ctx).await,
            Self::File { path, mode } => file::probe(path, *mode, ctx).await,
            #[cfg(feature = "http")]
            Self::Http { url, expect } => http::probe(url, expect, ctx).await,
            #[cfg(not(feature = "http"))]
            Self::Http { .. } => disabled_stage(StageKind::Http, "http"),
            #[cfg(feature = "postgres")]
            Self::Postgres { url, expect_table } => {
                postgres::probe(url, expect_table.as_deref(), ctx).await
            }
            #[cfg(not(feature = "postgres"))]
            Self::Postgres { .. } => disabled_stage(StageKind::Postgres, "postgres"),
            #[cfg(feature = "redis")]
            Self::Redis { url, expect_key } => redis::probe(url, expect_key.as_ref(), ctx).await,
            #[cfg(not(feature = "redis"))]
            Self::Redis { .. } => disabled_stage(StageKind::Redis, "redis"),
            #[cfg(feature = "mysql")]
            Self::Mysql { url, expect_table } => {
                mysql::probe(url, expect_table.as_deref(), ctx).await
            }
            #[cfg(not(feature = "mysql"))]
            Self::Mysql { .. } => disabled_stage(StageKind::Mysql, "mysql"),
            #[cfg(feature = "grpc")]
            Self::Grpc { url, service } => grpc::probe(url, service, ctx).await,
            #[cfg(not(feature = "grpc"))]
            Self::Grpc { .. } => disabled_stage(StageKind::Grpc, "grpc"),
            Self::Log { path, matcher } => log::probe(path, matcher).await,
            Self::Exec { program, args } => exec::probe(program, args, ctx).await,
            #[cfg(feature = "influxdb")]
            Self::Influxdb { url } => influxdb::probe(url, ctx).await,
            #[cfg(not(feature = "influxdb"))]
            Self::Influxdb { .. } => disabled_stage(StageKind::Influxdb, "influxdb"),
            #[cfg(feature = "mongodb")]
            Self::Mongodb { url } => mongodb::probe(url, ctx).await,
            #[cfg(not(feature = "mongodb"))]
            Self::Mongodb { .. } => disabled_stage(StageKind::Mongodb, "mongodb"),
            #[cfg(feature = "rabbitmq")]
            Self::Rabbitmq {
                url,
                queue,
                exchange,
            } => rabbitmq::probe(url, queue.as_deref(), exchange.as_deref(), ctx).await,
            #[cfg(not(feature = "rabbitmq"))]
            Self::Rabbitmq { .. } => disabled_stage(StageKind::Rabbitmq, "rabbitmq"),
            #[cfg(feature = "kafka")]
            Self::Kafka {
                url,
                topic,
                min_partitions,
            } => kafka::probe(url, topic.as_deref(), *min_partitions, ctx).await,
            #[cfg(not(feature = "kafka"))]
            Self::Kafka { .. } => disabled_stage(StageKind::Kafka, "kafka"),
            #[cfg(feature = "temporal")]
            Self::Temporal { url } => temporal::probe(url, ctx).await,
            #[cfg(not(feature = "temporal"))]
            Self::Temporal { .. } => disabled_stage(StageKind::Temporal, "temporal"),
            #[cfg(feature = "docker")]
            Self::Docker { name, expect } => docker::probe(name, expect, ctx).await,
            #[cfg(not(feature = "docker"))]
            Self::Docker { .. } => disabled_stage(StageKind::Docker, "docker"),
            #[cfg(feature = "k8s")]
            Self::K8s {
                kind,
                namespace,
                name,
            } => k8s::probe(*kind, namespace, name, ctx).await,
            #[cfg(not(feature = "k8s"))]
            Self::K8s { .. } => disabled_stage(StageKind::K8s, "k8s"),
            #[cfg(feature = "websocket")]
            Self::Ws { url } => websocket::probe(url, ctx).await,
            #[cfg(not(feature = "websocket"))]
            Self::Ws { .. } => disabled_stage(StageKind::Ws, "websocket"),
        };
        let ok = stages
            .last()
            .is_some_and(|s| matches!(s.result, StageResult::Ok));
        if ok {
            CheckOutcome::ready(stages, start.elapsed())
        } else {
            CheckOutcome::failed(stages, start.elapsed())
        }
    }
}

#[allow(dead_code)]
fn disabled_stage(kind: StageKind, feature: &str) -> Vec<Stage> {
    vec![Stage {
        kind,
        took: Duration::ZERO,
        result: StageResult::Err {
            message: format!("{feature} feature disabled").into(),
            hint: Some(format!("rebuild with --features {feature}").into()),
        },
    }]
}

pub(crate) fn err_stage(
    kind: StageKind,
    took: Duration,
    message: impl Into<Box<str>>,
    hint: Option<&str>,
) -> Stage {
    Stage {
        kind,
        took,
        result: StageResult::Err {
            message: message.into(),
            hint: hint.map(Into::into),
        },
    }
}

#[cfg(any(
    feature = "mysql",
    feature = "mongodb",
    feature = "rabbitmq",
    feature = "kafka"
))]
pub(crate) fn install_rustls_provider_once() {
    use std::sync::OnceLock;
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub(crate) const fn ok_stage(kind: StageKind, took: Duration) -> Stage {
    Stage {
        kind,
        took,
        result: StageResult::Ok,
    }
}

#[allow(dead_code)]
pub(crate) async fn run_stage<F, T, E>(
    kind: StageKind,
    attempt_timeout: Duration,
    timeout_hint: &'static str,
    fut: F,
    secrets: &[&str],
) -> Stage
where
    F: Future<Output = Result<T, E>>,
    E: std::error::Error + Hintable,
{
    let start = Instant::now();
    match timeout(attempt_timeout, fut).await {
        Ok(Ok(_)) => ok_stage(kind, start.elapsed()),
        Ok(Err(e)) => {
            let mut msg = format_error_chain(&e);
            if secrets.is_empty() {
                msg = crate::util::redact_userinfo(&msg);
            } else {
                for s in secrets {
                    msg = crate::util::redact_in(&msg, s);
                }
            }
            let h = e.hint();
            err_stage(kind, start.elapsed(), msg, h)
        }
        Err(_) => err_stage(kind, attempt_timeout, hints::TIMED_OUT, Some(timeout_hint)),
    }
}

#[cfg_attr(
    not(any(feature = "postgres", feature = "mysql", feature = "redis")),
    allow(dead_code)
)]
pub(crate) fn strip_query_keys(url: &url::Url, drop: &[&str]) -> url::Url {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !drop.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    if kept.is_empty() {
        out.set_query(None);
    } else {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish();
        out.set_query(Some(&q));
    }
    out
}
