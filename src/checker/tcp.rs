use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::format_error_chain;

const TCP_STAGE_COUNT: usize = 2;
const TCP_REMAINING_FLOOR: Duration = Duration::from_millis(50);

pub(super) async fn probe(host: &str, port: u16, ctx: AttemptCtx) -> Vec<Stage> {
    let mut stages = Vec::with_capacity(TCP_STAGE_COUNT);

    let dns_start = Instant::now();
    let host_owned = host.to_owned();
    let resolve = tokio::task::spawn_blocking(move || {
        (host_owned.as_str(), port)
            .to_socket_addrs()
            .map(Iterator::collect::<Vec<_>>)
    });
    let addrs = match timeout(ctx.attempt_timeout, resolve).await {
        Ok(Ok(Ok(a))) if !a.is_empty() => a,
        Ok(Ok(Ok(_))) => {
            stages.push(err_stage(
                StageKind::Dns,
                dns_start.elapsed(),
                "no addresses returned",
                None,
            ));
            return stages;
        }
        Ok(Ok(Err(e))) => {
            stages.push(err_stage(
                StageKind::Dns,
                dns_start.elapsed(),
                format_error_chain(&e),
                Some(hints::DNS_HINT),
            ));
            return stages;
        }
        Ok(Err(e)) => {
            stages.push(err_stage(
                StageKind::Dns,
                dns_start.elapsed(),
                format!("resolver task: {e}"),
                None,
            ));
            return stages;
        }
        Err(_) => {
            stages.push(err_stage(
                StageKind::Dns,
                ctx.attempt_timeout,
                hints::TIMED_OUT,
                Some("DNS server slow or unreachable"),
            ));
            return stages;
        }
    };
    stages.push(ok_stage(StageKind::Dns, dns_start.elapsed()));

    let tcp_start = Instant::now();
    let remaining = ctx
        .attempt_timeout
        .saturating_sub(dns_start.elapsed())
        .max(TCP_REMAINING_FLOOR);
    match timeout(remaining, TcpStream::connect(addrs.as_slice())).await {
        Ok(Ok(_)) => stages.push(ok_stage(StageKind::Tcp, tcp_start.elapsed())),
        Ok(Err(e)) => {
            stages.push(err_stage(
                StageKind::Tcp,
                tcp_start.elapsed(),
                format_error_chain(&e),
                e.hint(),
            ));
        }
        Err(_) => stages.push(err_stage(
            StageKind::Tcp,
            remaining,
            hints::TIMED_OUT,
            Some(hints::PORT_CLOSED),
        )),
    }
    stages
}
