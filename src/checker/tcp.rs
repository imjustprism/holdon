use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::LogMatcher;
use crate::util::{format_error_chain, sanitize_for_terminal};

const TCP_STAGE_COUNT: usize = 3;
const TCP_REMAINING_FLOOR: Duration = Duration::from_millis(50);
const BANNER_READ_CAP: usize = 4096;

#[allow(clippy::too_many_lines)]
pub(super) async fn probe(
    host: &str,
    port: u16,
    expect: Option<&LogMatcher>,
    ctx: AttemptCtx,
) -> Vec<Stage> {
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
                sanitize_for_terminal(&format!("resolver task: {e}")),
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
    let stream = match timeout(remaining, TcpStream::connect(addrs.as_slice())).await {
        Ok(Ok(s)) => {
            stages.push(ok_stage(StageKind::Tcp, tcp_start.elapsed()));
            s
        }
        Ok(Err(e)) => {
            stages.push(err_stage(
                StageKind::Tcp,
                tcp_start.elapsed(),
                format_error_chain(&e),
                e.hint(),
            ));
            return stages;
        }
        Err(_) => {
            stages.push(err_stage(
                StageKind::Tcp,
                remaining,
                hints::TIMED_OUT,
                Some(hints::PORT_CLOSED),
            ));
            return stages;
        }
    };
    let Some(matcher) = expect else {
        return stages;
    };
    let banner_start = Instant::now();
    let banner_budget = ctx
        .attempt_timeout
        .saturating_sub(dns_start.elapsed())
        .max(TCP_REMAINING_FLOOR);
    let banner_stage = match timeout(banner_budget, read_and_match(stream, matcher)).await {
        Ok(Ok(true)) => ok_stage(StageKind::Tcp, banner_start.elapsed()),
        Ok(Ok(false)) => err_stage(
            StageKind::Tcp,
            banner_start.elapsed(),
            "received banner did not match the expected pattern",
            Some(hints::TCP_BANNER_MISMATCH),
        ),
        Ok(Err(e)) => err_stage(
            StageKind::Tcp,
            banner_start.elapsed(),
            sanitize_for_terminal(&format!("reading banner: {}", format_error_chain(&e))),
            Some(hints::TCP_NO_BANNER),
        ),
        Err(_) => err_stage(
            StageKind::Tcp,
            banner_budget,
            hints::TIMED_OUT,
            Some(hints::TCP_NO_BANNER),
        ),
    };
    stages.push(banner_stage);
    stages
}

async fn read_and_match(mut stream: TcpStream, matcher: &LogMatcher) -> std::io::Result<bool> {
    let mut buf = Vec::with_capacity(BANNER_READ_CAP);
    let mut chunk = [0u8; 1024];
    loop {
        if matches_buf(&buf, matcher) {
            return Ok(true);
        }
        if buf.len() >= BANNER_READ_CAP {
            return Ok(false);
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(matches_buf(&buf, matcher));
        }
        let take = n.min(BANNER_READ_CAP - buf.len());
        buf.extend_from_slice(&chunk[..take]);
    }
}

fn matches_buf(buf: &[u8], matcher: &LogMatcher) -> bool {
    if buf.is_empty() {
        return false;
    }
    let text = String::from_utf8_lossy(buf);
    match matcher {
        LogMatcher::Substring(s) => text.contains(s.as_str()),
        LogMatcher::Regex(re) => re.is_match(&text),
    }
}
