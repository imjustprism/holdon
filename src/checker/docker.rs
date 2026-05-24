use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::{DockerExpect, LogMatcher};
use crate::util::sanitize_for_terminal;

const INSPECT_READ_CAP: usize = 64 * 1024;
const LOG_READ_CAP: usize = 2 * 1024 * 1024;
const QUERY_VALUE_SAFE: &percent_encoding::AsciiSet = &PATH_SAFE.add(b'&').add(b'+').add(b'=');

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error("connecting to Docker engine: {0}")]
    Connect(String),
    #[error("Docker engine API: {0}")]
    Protocol(String),
    #[error("container `{0}` not found")]
    NotFound(String),
    #[error("container `{name}` is `{actual}`, expected `{expected}`")]
    WrongState {
        name: String,
        actual: String,
        expected: String,
    },
    #[error("container `{0}` has no HEALTHCHECK defined")]
    NoHealthcheck(String),
    #[error("container `{name}` healthcheck is `{status}`")]
    NotHealthy { name: String, status: String },
    #[error("container `{0}` logs do not yet match the expected pattern")]
    LogMismatch(String),
    #[error("container `{0}` log tail exceeded the 2 MiB read cap")]
    LogTooLarge(String),
    #[error("Docker engine response exceeded the read cap")]
    CapExceeded,
    #[error("no docker compose container with label `com.docker.compose.service={0}`")]
    ComposeNoMatch(String),
}

impl ProbeError {
    const fn hint(&self) -> &'static str {
        match self {
            Self::Connect(_) => hints::DOCKER_NO_SOCKET,
            Self::Protocol(_) | Self::CapExceeded => hints::DOCKER_PROTOCOL,
            Self::NotFound(_) => hints::DOCKER_NO_SUCH,
            Self::WrongState { .. } => hints::DOCKER_NOT_RUNNING,
            Self::NoHealthcheck(_) => hints::DOCKER_NO_HEALTHCHECK,
            Self::NotHealthy { .. } => hints::DOCKER_UNHEALTHY,
            Self::LogMismatch(_) => hints::DOCKER_LOG_NOT_FOUND,
            Self::LogTooLarge(_) => hints::DOCKER_LOG_TOO_LARGE,
            Self::ComposeNoMatch(_) => hints::DOCKER_COMPOSE_NO_MATCH,
        }
    }
}

pub(super) async fn probe(name: &str, expect: &DockerExpect, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let stage = match timeout(ctx.attempt_timeout, run(name, expect)).await {
        Ok(Ok(())) => ok_stage(StageKind::Docker, start.elapsed()),
        Ok(Err(e)) => {
            let h = e.hint();
            err_stage(
                StageKind::Docker,
                start.elapsed(),
                sanitize_for_terminal(&e.to_string()),
                Some(h),
            )
        }
        Err(_) => err_stage(
            StageKind::Docker,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::DOCKER_NO_SOCKET),
        ),
    };
    vec![stage]
}

pub(super) async fn probe_compose(
    service: &str,
    expect: &DockerExpect,
    ctx: AttemptCtx,
) -> Vec<Stage> {
    let start = Instant::now();
    let stage = match timeout(ctx.attempt_timeout, run_compose(service, expect)).await {
        Ok(Ok(())) => ok_stage(StageKind::Docker, start.elapsed()),
        Ok(Err(e)) => {
            let h = e.hint();
            err_stage(
                StageKind::Docker,
                start.elapsed(),
                sanitize_for_terminal(&e.to_string()),
                Some(h),
            )
        }
        Err(_) => err_stage(
            StageKind::Docker,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::DOCKER_NO_SOCKET),
        ),
    };
    vec![stage]
}

async fn run_compose(service: &str, expect: &DockerExpect) -> Result<(), ProbeError> {
    let preferred_state = expect.state.as_deref().unwrap_or("running");
    let name = resolve_compose_container(service, preferred_state).await?;
    run(&name, expect).await
}

async fn resolve_compose_container(
    service: &str,
    preferred_state: &str,
) -> Result<String, ProbeError> {
    let label = format!("com.docker.compose.service={service}");
    let filter_value = format!(r#"{{"label":[{}]}}"#, encode_json_string(&label));
    let encoded =
        percent_encoding::utf8_percent_encode(&filter_value, QUERY_VALUE_SAFE).to_string();
    let request = format!(
        "GET /containers/json?all=true&filters={encoded} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: holdon/{ver}\r\n\r\n",
        ver = env!("CARGO_PKG_VERSION"),
    );
    let raw = transport::round_trip(request.as_bytes(), INSPECT_READ_CAP).await?;
    let (status, body) = parse_response(&raw)?;
    if status != 200 {
        return Err(ProbeError::Protocol(format!(
            "unexpected HTTP {status} from /containers/json filter for `{service}`"
        )));
    }
    let containers: Vec<ComposeContainer> = serde_json::from_str(&body)
        .map_err(|e| ProbeError::Protocol(format!("invalid JSON from /containers/json: {e}")))?;
    let Some(picked) = containers
        .iter()
        .find(|c| c.state.eq_ignore_ascii_case(preferred_state))
        .or_else(|| containers.first())
    else {
        return Err(ProbeError::ComposeNoMatch(service.to_owned()));
    };
    let name = picked
        .names
        .iter()
        .find_map(|n| n.strip_prefix('/').map(str::to_owned))
        .or_else(|| picked.names.first().cloned())
        .unwrap_or_else(|| picked.id.clone());
    Ok(name)
}

fn encode_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, serde::Deserialize)]
struct ComposeContainer {
    #[serde(rename = "Id", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "State", default)]
    state: String,
}

async fn run(name: &str, expect: &DockerExpect) -> Result<(), ProbeError> {
    let want_state = expect.state.as_deref().unwrap_or("running");
    let body = inspect(name).await?;
    let parsed: InspectResponse = serde_json::from_str(&body).map_err(|e| {
        ProbeError::Protocol(format!("invalid JSON from /containers/{name}/json: {e}"))
    })?;
    if !parsed.state.status.eq_ignore_ascii_case(want_state) {
        return Err(ProbeError::WrongState {
            name: name.to_owned(),
            actual: parsed.state.status,
            expected: want_state.to_owned(),
        });
    }
    if expect.require_healthy {
        let Some(health) = parsed.state.health else {
            return Err(ProbeError::NoHealthcheck(name.to_owned()));
        };
        if !health.status.eq_ignore_ascii_case("healthy") {
            return Err(ProbeError::NotHealthy {
                name: name.to_owned(),
                status: health.status,
            });
        }
    }
    if let Some(matcher) = &expect.log_match {
        let raw = fetch_logs(name).await?;
        let text = demux_logs(&raw);
        let hit = match matcher {
            LogMatcher::Substring(s) => text.contains(s.as_str()),
            LogMatcher::Regex(re) => re.is_match(&text),
        };
        if !hit {
            return Err(ProbeError::LogMismatch(name.to_owned()));
        }
    }
    Ok(())
}

const LOG_TAIL: usize = 200;

async fn fetch_logs(name: &str) -> Result<Vec<u8>, ProbeError> {
    let encoded = percent_encoding::utf8_percent_encode(name, PATH_SAFE).to_string();
    let request = format!(
        "GET /containers/{encoded}/logs?stdout=1&stderr=1&tail={tail} HTTP/1.1\r\nHost: docker\r\nAccept: */*\r\nConnection: close\r\nUser-Agent: holdon/{ver}\r\n\r\n",
        tail = LOG_TAIL,
        ver = env!("CARGO_PKG_VERSION"),
    );
    let raw = transport::round_trip(request.as_bytes(), LOG_READ_CAP)
        .await
        .map_err(|e| match e {
            ProbeError::CapExceeded => ProbeError::LogTooLarge(name.to_owned()),
            other => other,
        })?;
    let (status, body) = parse_response_bytes(&raw)?;
    match status {
        200 => Ok(body),
        404 => Err(ProbeError::NotFound(name.to_owned())),
        other => Err(ProbeError::Protocol(format!(
            "unexpected HTTP {other} from /containers/{name}/logs"
        ))),
    }
}

pub(crate) fn demux_logs(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if i + 8 > raw.len() {
            out.extend_from_slice(&raw[i..]);
            break;
        }
        let stream = raw[i];
        if stream > 2 || raw[i + 1] != 0 || raw[i + 2] != 0 || raw[i + 3] != 0 {
            out.extend_from_slice(&raw[i..]);
            break;
        }
        let size = u32::from_be_bytes([raw[i + 4], raw[i + 5], raw[i + 6], raw[i + 7]]) as usize;
        i += 8;
        let end = i.saturating_add(size).min(raw.len());
        out.extend_from_slice(&raw[i..end]);
        i = end;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn inspect(name: &str) -> Result<String, ProbeError> {
    let encoded = percent_encoding::utf8_percent_encode(name, PATH_SAFE).to_string();
    let request = format!(
        "GET /containers/{encoded}/json HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: holdon/{ver}\r\n\r\n",
        ver = env!("CARGO_PKG_VERSION"),
    );
    let raw = transport::round_trip(request.as_bytes(), INSPECT_READ_CAP).await?;
    let (status, body) = parse_response(&raw)?;
    match status {
        200 => Ok(body),
        404 => Err(ProbeError::NotFound(name.to_owned())),
        500 => Err(ProbeError::Protocol(format!(
            "engine reported 500: {}",
            extract_engine_message(&body)
        ))),
        other => Err(ProbeError::Protocol(format!(
            "unexpected HTTP {other} from /containers/{name}/json"
        ))),
    }
}

const PATH_SAFE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'?')
    .add(b'/')
    .add(b'\\');

fn parse_response_bytes(raw: &[u8]) -> Result<(u16, Vec<u8>), ProbeError> {
    let split = find_header_terminator(raw)
        .ok_or_else(|| ProbeError::Protocol("response missing CRLFCRLF separator".into()))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| ProbeError::Protocol("response headers were not valid UTF-8".into()))?;
    let body = &raw[split + 4..];
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ProbeError::Protocol("empty response".into()))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            ProbeError::Protocol(format!("could not parse status line `{status_line}`"))
        })?;
    let chunked = head
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .any(|(k, v)| {
            k.trim().eq_ignore_ascii_case("transfer-encoding")
                && v.split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("chunked"))
        });
    let body_bytes = if chunked {
        decode_chunked_bytes(body)?
    } else {
        body.to_vec()
    };
    Ok((status, body_bytes))
}

fn decode_chunked_bytes(body: &[u8]) -> Result<Vec<u8>, ProbeError> {
    let mut out = Vec::with_capacity(body.len());
    let mut cursor = 0;
    while cursor < body.len() {
        let line_end = body[cursor..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| ProbeError::Protocol("malformed chunked body".into()))?;
        let size_hex = std::str::from_utf8(&body[cursor..cursor + line_end])
            .map_err(|_| ProbeError::Protocol("chunk size not UTF-8".into()))?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| ProbeError::Protocol(format!("invalid chunk size `{size_hex}`")))?;
        cursor += line_end + 2;
        if size == 0 {
            break;
        }
        if cursor + size > body.len() {
            return Err(ProbeError::Protocol("chunked body truncated".into()));
        }
        out.extend_from_slice(&body[cursor..cursor + size]);
        cursor += size;
        if cursor + 2 > body.len() || &body[cursor..cursor + 2] != b"\r\n" {
            return Err(ProbeError::Protocol("chunk missing trailing CRLF".into()));
        }
        cursor += 2;
    }
    Ok(out)
}

fn parse_response(raw: &[u8]) -> Result<(u16, String), ProbeError> {
    let (status, body) = parse_response_bytes(raw)?;
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn find_header_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn extract_engine_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .unwrap_or_else(|| body.chars().take(160).collect())
}

#[derive(Debug, serde::Deserialize)]
struct InspectResponse {
    #[serde(rename = "State")]
    state: State,
}

#[derive(Debug, serde::Deserialize)]
struct State {
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Health", default)]
    health: Option<Health>,
}

#[derive(Debug, serde::Deserialize)]
struct Health {
    #[serde(rename = "Status")]
    status: String,
}

#[cfg(unix)]
mod transport {
    use std::path::PathBuf;

    use super::ProbeError;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;

    pub(super) async fn round_trip(request: &[u8], cap: usize) -> Result<Vec<u8>, ProbeError> {
        let path = socket_path()?;
        let mut stream = UnixStream::connect(&path).await.map_err(|e| {
            ProbeError::Connect(format!(
                "opening {}: {}",
                path.display(),
                super::io_kind(&e)
            ))
        })?;
        stream
            .write_all(request)
            .await
            .map_err(|e| ProbeError::Connect(format!("writing request: {}", super::io_kind(&e))))?;
        stream
            .shutdown()
            .await
            .map_err(|e| ProbeError::Connect(format!("shutdown: {}", super::io_kind(&e))))?;
        super::read_capped(&mut stream, cap).await
    }

    fn socket_path() -> Result<PathBuf, ProbeError> {
        match std::env::var("DOCKER_HOST") {
            Ok(host) if host.is_empty() => Ok(PathBuf::from("/var/run/docker.sock")),
            Ok(host) => host.strip_prefix("unix://").map_or_else(
                || {
                    Err(ProbeError::Connect(format!(
                        "DOCKER_HOST scheme not supported by holdon (`{host}`), only unix:// is implemented on this platform"
                    )))
                },
                |rest| Ok(PathBuf::from(rest)),
            ),
            Err(_) => Ok(PathBuf::from("/var/run/docker.sock")),
        }
    }
}

#[cfg(windows)]
mod transport {
    use super::ProbeError;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    pub(super) async fn round_trip(request: &[u8], cap: usize) -> Result<Vec<u8>, ProbeError> {
        let path = pipe_path()?;
        let mut stream = ClientOptions::new()
            .open(&path)
            .map_err(|e| ProbeError::Connect(format!("opening {path}: {}", super::io_kind(&e))))?;
        stream
            .write_all(request)
            .await
            .map_err(|e| ProbeError::Connect(format!("writing request: {}", super::io_kind(&e))))?;
        super::read_capped(&mut stream, cap).await
    }

    fn pipe_path() -> Result<String, ProbeError> {
        match std::env::var("DOCKER_HOST") {
            Ok(host) if host.is_empty() => Ok(r"\\.\pipe\docker_engine".to_owned()),
            Ok(host) => host.strip_prefix("npipe://").map_or_else(
                || {
                    Err(ProbeError::Connect(format!(
                        "DOCKER_HOST scheme not supported by holdon (`{host}`), only npipe:// is implemented on this platform"
                    )))
                },
                |rest| Ok(rest.replace('/', "\\")),
            ),
            Err(_) => Ok(r"\\.\pipe\docker_engine".to_owned()),
        }
    }
}

async fn read_capped<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    cap: usize,
) -> Result<Vec<u8>, ProbeError> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        if buf.len() == cap {
            let mut overflow = [0u8; 1];
            return match reader.read(&mut overflow).await {
                Ok(0) => Ok(buf),
                Ok(_) => Err(ProbeError::CapExceeded),
                Err(e) => Err(ProbeError::Connect(format!(
                    "reading response: {}",
                    io_kind(&e)
                ))),
            };
        }
        let remaining = cap - buf.len();
        let take_max = remaining.min(chunk.len());
        match reader.read(&mut chunk[..take_max]).await {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => {
                return Err(ProbeError::Connect(format!(
                    "reading response: {}",
                    io_kind(&e)
                )));
            }
        }
    }
    Ok(buf)
}

fn io_kind(e: &std::io::Error) -> String {
    format!("{e}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_response() {
        let raw =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"State\":{\"Status\":\"running\"}}";
        let (status, body) = parse_response(raw).unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("running"));
    }

    #[test]
    fn parses_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1e\r\n{\"State\":{\"Status\":\"running\"}}\r\n0\r\n\r\n";
        let (status, body) = parse_response(raw).unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("running"));
    }

    #[test]
    fn rejects_missing_terminator() {
        let raw = b"HTTP/1.1 200 OK\r\nno terminator";
        assert!(parse_response(raw).is_err());
    }

    #[test]
    fn demux_strips_multiplex_headers() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 5]);
        raw.extend_from_slice(b"hello");
        raw.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 6]);
        raw.extend_from_slice(b" world");
        let text = demux_logs(&raw);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn demux_passes_tty_streams_through() {
        let raw = b"plain text without frame headers";
        let text = demux_logs(raw);
        assert_eq!(text, "plain text without frame headers");
    }

    #[test]
    fn demux_handles_truncated_frame() {
        let mut raw = vec![1, 0, 0, 0, 0, 0, 0, 99];
        raw.extend_from_slice(b"short");
        let text = demux_logs(&raw);
        assert_eq!(text, "short");
    }

    #[test]
    fn deserialise_with_health() {
        let body = r#"{"State":{"Status":"running","Health":{"Status":"healthy"}}}"#;
        let v: InspectResponse = serde_json::from_str(body).unwrap();
        assert_eq!(v.state.status, "running");
        assert_eq!(v.state.health.unwrap().status, "healthy");
    }

    #[test]
    fn deserialise_without_health() {
        let body = r#"{"State":{"Status":"running"}}"#;
        let v: InspectResponse = serde_json::from_str(body).unwrap();
        assert!(v.state.health.is_none());
    }

    #[test]
    fn engine_message_extraction() {
        let body = r#"{"message":"No such container: abc"}"#;
        assert_eq!(extract_engine_message(body), "No such container: abc");
    }
}
