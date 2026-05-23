use std::net::{IpAddr, ToSocketAddrs};
use std::time::Instant;

use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, sanitize_for_terminal};

pub(super) async fn probe(host: &str, expect_ip: Option<IpAddr>, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let host_owned = host.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        (host_owned.as_str(), 0u16)
            .to_socket_addrs()
            .map(|it| it.map(|s| s.ip()).collect::<Vec<_>>())
    });
    let stage = match timeout(ctx.attempt_timeout, task).await {
        Ok(Ok(Ok(ips))) if !ips.is_empty() => match expect_ip {
            None => ok_stage(StageKind::Dns, start.elapsed()),
            Some(want) if ips.contains(&want) => ok_stage(StageKind::Dns, start.elapsed()),
            Some(want) => {
                let got: Vec<String> = ips.iter().map(ToString::to_string).collect();
                err_stage(
                    StageKind::Dns,
                    start.elapsed(),
                    format!("resolved to {} but expected {want}", got.join(", ")),
                    Some(hints::DNS_EXPECT_MISMATCH),
                )
            }
        },
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
            sanitize_for_terminal(&format!("resolver task: {e}")),
            None,
        ),
        Err(_) => err_stage(StageKind::Dns, ctx.attempt_timeout, hints::TIMED_OUT, None),
    };
    vec![stage]
}
