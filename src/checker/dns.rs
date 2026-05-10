use std::net::ToSocketAddrs;
use std::time::Instant;

use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::format_error_chain;

pub(super) async fn probe(host: &str, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let host_owned = host.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        (host_owned.as_str(), 0u16)
            .to_socket_addrs()
            .map(Iterator::count)
    });
    let stage = match timeout(ctx.attempt_timeout, task).await {
        Ok(Ok(Ok(n))) if n > 0 => ok_stage(StageKind::Dns, start.elapsed()),
        Ok(Ok(Ok(_))) => err_stage(StageKind::Dns, start.elapsed(), "no addresses", None),
        Ok(Ok(Err(e))) => err_stage(
            StageKind::Dns,
            start.elapsed(),
            format_error_chain(&e),
            Some(hints::DNS_HINT),
        ),
        Ok(Err(e)) => err_stage(
            StageKind::Dns,
            start.elapsed(),
            format!("resolver task: {e}"),
            None,
        ),
        Err(_) => err_stage(StageKind::Dns, ctx.attempt_timeout, hints::TIMED_OUT, None),
    };
    vec![stage]
}
