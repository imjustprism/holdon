use std::future::Future;
use std::time::{Duration, Instant};

use tokio::time::timeout;

use crate::diagnostic::{CheckOutcome, Stage, StageKind, StageResult};
use crate::target::Target;
use crate::util::format_error_chain;

mod dns;
mod exec;
mod file;
mod hint;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "redis")]
mod redis;
mod tcp;

pub(crate) use hint::{Hintable, hints};

/// Per-attempt context passed from [`crate::Runner`] down to each checker.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AttemptCtx {
    /// Wall-clock budget for one probe attempt.
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
    /// Runs one probe attempt against this target.
    ///
    /// Dispatches to the protocol-specific checker (TCP, HTTP, Postgres, ...)
    /// based on the variant. Returns a [`CheckOutcome`] capturing every stage,
    /// the total wall time, and the final pass/fail status. Errors from
    /// upstream libraries are formatted via [`crate::util::format_error_chain`]
    /// which sanitizes control bytes, and any URL password is redacted before
    /// reaching the result.
    pub async fn probe(&self, ctx: AttemptCtx) -> CheckOutcome {
        let start = Instant::now();
        let stages = match self {
            Self::Tcp { host, port } => tcp::probe(host.as_str(), *port, ctx).await,
            Self::Dns { host } => dns::probe(host.as_str(), ctx).await,
            Self::File { path, mode } => file::probe(path, *mode).await,
            #[cfg(feature = "http")]
            Self::Http { url, expect } => http::probe(url, expect, ctx).await,
            #[cfg(not(feature = "http"))]
            Self::Http { .. } => disabled_stage(StageKind::Http, "http"),
            #[cfg(feature = "postgres")]
            Self::Postgres { url } => postgres::probe(url, ctx).await,
            #[cfg(not(feature = "postgres"))]
            Self::Postgres { .. } => disabled_stage(StageKind::Postgres, "postgres"),
            #[cfg(feature = "redis")]
            Self::Redis { url } => redis::probe(url, ctx).await,
            #[cfg(not(feature = "redis"))]
            Self::Redis { .. } => disabled_stage(StageKind::Redis, "redis"),
            Self::Mysql { .. } => disabled_stage(StageKind::Mysql, "mysql"),
            Self::Exec { program, args } => exec::probe(program, args, ctx).await,
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
            for s in secrets {
                if !s.is_empty() {
                    msg = msg.replace(*s, "***");
                }
            }
            let h = e.hint();
            err_stage(kind, start.elapsed(), msg, h)
        }
        Err(_) => err_stage(kind, attempt_timeout, hints::TIMED_OUT, Some(timeout_hint)),
    }
}
