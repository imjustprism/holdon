//! Docker container readiness probe.
//!
//! Talks to the Docker engine API over its local IPC socket (Unix socket on
//! Linux/macOS, named pipe on Windows) and inspects a single container.
//! Reports ready when the container reaches the expected `State.Status`
//! (default `running`) and, if `?healthy=true` was requested, also has a
//! healthcheck reporting `healthy`.
//!
//! The module deliberately avoids pulling in a heavy Docker client crate.
//! It speaks raw HTTP/1.1 over the engine socket because the only call we
//! need is `GET /containers/{name}/json`, which has a stable shape across
//! every supported engine version.

use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::DockerExpect;
use crate::util::sanitize_for_terminal;

const HTTP_READ_CAP: usize = 64 * 1024;

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
}

impl ProbeError {
    const fn hint(&self) -> &'static str {
        match self {
            Self::Connect(_) => hints::DOCKER_NO_SOCKET,
            Self::Protocol(_) => hints::DOCKER_PROTOCOL,
            Self::NotFound(_) => hints::DOCKER_NO_SUCH,
            Self::WrongState { .. } => hints::DOCKER_NOT_RUNNING,
            Self::NoHealthcheck(_) => hints::DOCKER_NO_HEALTHCHECK,
            Self::NotHealthy { .. } => hints::DOCKER_UNHEALTHY,
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
    Ok(())
}

async fn inspect(name: &str) -> Result<String, ProbeError> {
    let encoded = percent_encoding::utf8_percent_encode(name, PATH_SAFE).to_string();
    let request = format!(
        "GET /containers/{encoded}/json HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: holdon/{ver}\r\n\r\n",
        ver = env!("CARGO_PKG_VERSION"),
    );
    let raw = transport::round_trip(request.as_bytes()).await?;
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

/// Characters that must be percent-encoded when interpolated into the URL
/// path. Docker container names are restricted to `[a-zA-Z0-9_.-]`, but the
/// API also accepts container IDs (hex) and we want to fail explicitly with
/// a 404 rather than smuggling unexpected bytes into the request line.
const PATH_SAFE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'?')
    .add(b'/')
    .add(b'\\');

fn parse_response(raw: &[u8]) -> Result<(u16, String), ProbeError> {
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
    let body_str = if chunked {
        decode_chunked(body)?
    } else {
        String::from_utf8_lossy(body).into_owned()
    };
    Ok((status, body_str))
}

fn find_header_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn decode_chunked(body: &[u8]) -> Result<String, ProbeError> {
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
        // RFC 9112: each chunk ends with CRLF after the data. Validate it
        // explicitly so a truncation between chunks surfaces a precise
        // error instead of a downstream "invalid chunk size" message.
        if cursor + 2 > body.len() || &body[cursor..cursor + 2] != b"\r\n" {
            return Err(ProbeError::Protocol("chunk missing trailing CRLF".into()));
        }
        cursor += 2;
    }
    String::from_utf8(out).map_err(|_| ProbeError::Protocol("chunked body not UTF-8".into()))
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

    pub(super) async fn round_trip(request: &[u8]) -> Result<Vec<u8>, ProbeError> {
        let path = socket_path();
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
        super::read_capped(&mut stream).await
    }

    fn socket_path() -> PathBuf {
        if let Ok(host) = std::env::var("DOCKER_HOST") {
            if let Some(rest) = host.strip_prefix("unix://") {
                return PathBuf::from(rest);
            }
        }
        PathBuf::from("/var/run/docker.sock")
    }
}

#[cfg(windows)]
mod transport {
    use super::ProbeError;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    pub(super) async fn round_trip(request: &[u8]) -> Result<Vec<u8>, ProbeError> {
        let path = pipe_path();
        let mut stream = ClientOptions::new()
            .open(&path)
            .map_err(|e| ProbeError::Connect(format!("opening {path}: {}", super::io_kind(&e))))?;
        stream
            .write_all(request)
            .await
            .map_err(|e| ProbeError::Connect(format!("writing request: {}", super::io_kind(&e))))?;
        super::read_capped(&mut stream).await
    }

    fn pipe_path() -> String {
        if let Ok(host) = std::env::var("DOCKER_HOST") {
            if let Some(rest) = host.strip_prefix("npipe://") {
                return rest.replace('/', "\\");
            }
        }
        r"\\.\pipe\docker_engine".to_owned()
    }
}

async fn read_capped<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Vec<u8>, ProbeError> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        if buf.len() == HTTP_READ_CAP {
            // Buffer holds exactly the cap. Peek one more byte to tell an
            // exact-fit response apart from one that exceeds the cap and
            // would otherwise be silently truncated.
            let mut overflow = [0u8; 1];
            return match reader.read(&mut overflow).await {
                Ok(0) => Ok(buf),
                Ok(_) => Err(ProbeError::Protocol(format!(
                    "Docker engine response exceeded {HTTP_READ_CAP} bytes"
                ))),
                Err(e) => Err(ProbeError::Connect(format!(
                    "reading response: {}",
                    io_kind(&e)
                ))),
            };
        }
        let remaining = HTTP_READ_CAP - buf.len();
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
        // 0x1e = 30 = byte length of the JSON payload below
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
