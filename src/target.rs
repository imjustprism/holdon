use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use url::Url;

use crate::error::{Error, Result};

const HOSTNAME_MAX_LEN: usize = 253;

/// A validated hostname or IP literal.
///
/// Rejects empty strings, inputs over 253 bytes (RFC 1035 §2.3.4), and any
/// control bytes (`< 0x20`) or DEL (`0x7F`). Does not enforce DNS label syntax
/// beyond length and control-byte rejection because the same type is used for
/// IPv4 / IPv6 literals and DNS-only targets where the user intentionally
/// passes an unusual hostname. The OS resolver makes the final call.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Hostname(Box<str>);

impl Hostname {
    /// Builds a [`Hostname`], validating the input.
    ///
    /// # Errors
    /// Returns [`Error::Parse`] if the input is empty, too long, or contains
    /// control bytes.
    pub fn new(s: impl Into<Box<str>>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() {
            return Err(parse_err(&s, "empty hostname"));
        }
        if s.len() > HOSTNAME_MAX_LEN {
            return Err(parse_err(&s, "hostname exceeds 253 bytes"));
        }
        if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(parse_err(&s, "hostname contains control bytes"));
        }
        Ok(Self(s))
    }

    /// Returns the validated hostname as a string slice.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hostname({:?})", self.0)
    }
}

impl AsRef<str> for Hostname {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A parsed readiness target.
///
/// Construct via [`str::parse`] from a CLI-style string, then pass to
/// [`crate::Runner::run`].
///
/// Accepted shapes (see the [`FromStr`] impl for the full grammar):
/// `:5432`, `host:5432`, `[::1]:5432`, `tcp://host:5432`, `tcp:host:5432`,
/// `http(s)://...`, `postgres(ql)://...`, `redis(s)://...`, `mysql://...`,
/// `dns://host`, `file:///abs/path[?mode=absent]`, `exec://program[?arg=...]`.
///
/// `Display` redacts URL passwords to `***`. The custom `Debug` impl does the
/// same so `?target` in tracing or panic messages cannot leak secrets.
#[derive(Clone)]
#[non_exhaustive]
pub enum Target {
    /// Plain TCP connect to `host:port`.
    Tcp {
        /// Hostname or IP literal (without brackets for IPv6).
        host: Hostname,
        /// TCP port.
        port: u16,
    },
    /// HTTP(S) request, ready when status falls in `expect`.
    Http {
        /// Fully-parsed URL.
        url: Url,
        /// Status code range that counts as ready. Defaults to 2xx.
        expect: StatusRange,
    },
    /// Hostname resolution check, no socket opened.
    Dns {
        /// Hostname to resolve.
        host: Hostname,
    },
    /// Filesystem existence check.
    File {
        /// Absolute path. UNC and `\\?\` paths are refused at parse time.
        path: PathBuf,
        /// Whether to wait for the path to exist or to disappear.
        mode: FileMode,
    },
    /// Postgres connect plus `SELECT 1`.
    Postgres {
        /// Full Postgres connection URL.
        url: Url,
    },
    /// Redis connect plus `PING`.
    Redis {
        /// Full Redis connection URL.
        url: Url,
    },
    /// `MySQL` / `MariaDB` connect + `SELECT 1`. TLS by default (rustls), opt
    /// out with `?ssl-mode=disable`. Behind the `mysql` feature.
    Mysql {
        /// Full `MySQL` connection URL.
        url: Url,
    },
    /// Run an external command, ready iff it exits 0.
    ///
    /// Parsed from `exec://<program>[?arg=...&arg=...]`. The program may be a
    /// bare executable name (resolved via the OS PATH), a relative path
    /// (refused by default, opt-in via `HOLDON_ALLOW_RELATIVE_EXEC=1`), or an absolute path
    /// (`exec:///usr/local/bin/pg_isready`). Arguments are passed via repeated
    /// `arg=` query parameters and are never interpreted by a shell.
    ///
    /// `exec://` runs whatever the user told it to. Treat target strings as
    /// code at the invocation site.
    Exec {
        /// Program path or name. Passed verbatim to
        /// [`tokio::process::Command::new`].
        program: PathBuf,
        /// Arguments, in order. Never split or shell-expanded.
        args: Vec<String>,
    },
    /// gRPC `grpc.health.v1.Health/Check` against the target endpoint.
    ///
    /// Parsed from `grpc://host:port[/Service]` (plaintext) or
    /// `grpcs://host:port[/Service]` (TLS via rustls). Path component selects
    /// the gRPC service name to check. Empty path means overall server health.
    /// Behind the `grpc` feature.
    Grpc {
        /// Endpoint URL (`grpc://` or `grpcs://`).
        url: Url,
        /// gRPC service name passed in `HealthCheckRequest.service`. Empty
        /// string means "overall server" per the protocol.
        service: String,
    },
}

/// Whether a [`Target::File`] check waits for a path to exist or to disappear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileMode {
    /// Wait until the path exists. Default.
    #[default]
    Present,
    /// Wait until the path no longer exists. Selected via `?mode=absent`.
    Absent,
}

/// Inclusive HTTP status code range that counts as ready.
///
/// Default is `200..=299` (strict 2xx). 3xx are not considered ready by
/// default to avoid masking misconfigured load balancers and edge redirects.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusRange {
    lo: u16,
    hi: u16,
}

const HTTP_2XX_LO: u16 = 200;
const HTTP_2XX_HI: u16 = 299;

impl StatusRange {
    /// Returns the default `200..=299` range.
    #[must_use]
    pub const fn ok_2xx() -> Self {
        Self {
            lo: HTTP_2XX_LO,
            hi: HTTP_2XX_HI,
        }
    }

    /// Builds a custom inclusive range. Caller must pass `lo <= hi`.
    #[must_use]
    pub const fn new(lo: u16, hi: u16) -> Self {
        Self {
            lo,
            hi: if hi < lo { lo } else { hi },
        }
    }

    /// Returns `true` if `status` falls inside the inclusive range.
    #[must_use]
    pub const fn contains(&self, status: u16) -> bool {
        status >= self.lo && status <= self.hi
    }
}

impl Default for StatusRange {
    fn default() -> Self {
        Self::ok_2xx()
    }
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp { host, port } => f
                .debug_struct("Tcp")
                .field("host", &host.as_str())
                .field("port", port)
                .finish(),
            Self::Http { url, expect } => f
                .debug_struct("Http")
                .field("url", &redact(url))
                .field("expect", expect)
                .finish(),
            Self::Dns { host } => f.debug_struct("Dns").field("host", &host.as_str()).finish(),
            Self::File { path, mode } => f
                .debug_struct("File")
                .field("path", path)
                .field("mode", mode)
                .finish(),
            Self::Postgres { url } => f
                .debug_struct("Postgres")
                .field("url", &redact(url))
                .finish(),
            Self::Redis { url } => f.debug_struct("Redis").field("url", &redact(url)).finish(),
            Self::Mysql { url } => f.debug_struct("Mysql").field("url", &redact(url)).finish(),
            Self::Grpc { url, service } => f
                .debug_struct("Grpc")
                .field("url", &redact(url))
                .field("service", service)
                .finish(),
            Self::Exec { program, args } => f
                .debug_struct("Exec")
                .field("program", program)
                .field("args", args)
                .finish(),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp { host, port } => write!(f, "tcp://{host}:{port}"),
            Self::Http { url, .. } => write!(f, "{url}"),
            Self::Dns { host } => write!(f, "dns://{host}"),
            Self::File { path, mode } => match mode {
                FileMode::Present => write!(f, "file://{}", path.display()),
                FileMode::Absent => write!(f, "file://{}?mode=absent", path.display()),
            },
            Self::Postgres { url } | Self::Redis { url } | Self::Mysql { url } => {
                write!(f, "{}", redact(url))
            }
            Self::Grpc { url, .. } => write!(f, "{}", redact(url)),
            Self::Exec { program, args } => {
                write!(f, "exec://{}", program.display())?;
                let mut first = true;
                for a in args {
                    f.write_str(if first { "?" } else { "&" })?;
                    first = false;
                    write!(f, "arg={}", encode_arg(a))?;
                }
                Ok(())
            }
        }
    }
}

fn encode_arg(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' | '=' | '?' | '#' | '%' | ' ' => {
                for b in c.to_string().bytes() {
                    let _ = write!(out, "%{b:02X}");
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn is_unc_or_remote(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(r"\\") || s.starts_with("//")
}

fn redact(url: &Url) -> String {
    if url.password().is_none() {
        return url.to_string();
    }
    let mut clone = url.clone();
    let _ = clone.set_password(Some("***"));
    clone.to_string()
}

impl FromStr for Target {
    type Err = Error;

    #[allow(clippy::too_many_lines)]
    fn from_str(input: &str) -> Result<Self> {
        let lower = input.to_ascii_lowercase();
        if lower.starts_with("file:////") || lower.starts_with("file:\\\\") {
            return Err(parse_err(
                input,
                "remote/UNC file paths are refused (NTLM-relay risk)",
            ));
        }
        if let Some(rest) = input.strip_prefix(':') {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                let port = parse_port(rest, input)?;
                return Ok(Self::Tcp {
                    host: Hostname::new("localhost")?,
                    port,
                });
            }
        }

        if let Some(rest) = input.strip_prefix("exec://") {
            return parse_exec_target(rest, input);
        }

        if !input.contains("://") {
            if let Some(rest) = input.strip_prefix('[') {
                let (host, port) = rest
                    .split_once("]:")
                    .ok_or_else(|| parse_err(input, "expected `[ipv6]:port`"))?;
                let port = parse_port(port, input)?;
                return Ok(Self::Tcp {
                    host: Hostname::new(host)?,
                    port,
                });
            }
            if let Some(rest) = input.strip_prefix("tcp:") {
                if !rest.starts_with("//") {
                    let (host, port) = rest
                        .rsplit_once(':')
                        .ok_or_else(|| Error::MissingPort(input.into()))?;
                    let port = parse_port(port, input)?;
                    return Ok(Self::Tcp {
                        host: Hostname::new(host)?,
                        port,
                    });
                }
            }
            if input.matches(':').count() > 1 {
                return Err(parse_err(
                    input,
                    "ambiguous IPv6, wrap in brackets like `[::1]:port`",
                ));
            }
            let (host, port) = input
                .rsplit_once(':')
                .ok_or_else(|| Error::MissingPort(input.into()))?;
            let port = parse_port(port, input)?;
            return Ok(Self::Tcp {
                host: Hostname::new(host)?,
                port,
            });
        }

        let url = Url::parse(input)?;
        match url.scheme() {
            "tcp" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                let port = url.port().ok_or_else(|| Error::MissingPort(input.into()))?;
                Ok(Self::Tcp {
                    host: Hostname::new(host)?,
                    port,
                })
            }
            "http" | "https" => Ok(Self::Http {
                url,
                expect: StatusRange::default(),
            }),
            "postgres" | "postgresql" => Ok(Self::Postgres { url }),
            "redis" | "rediss" => Ok(Self::Redis { url }),
            "mysql" | "mariadb" => Ok(Self::Mysql { url }),
            "grpc" | "grpcs" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port_or_known_default()
                    .ok_or_else(|| Error::MissingPort(input.into()))?;
                let raw = url.path().trim_start_matches('/').trim_end_matches('/');
                let service = if raw.is_empty() {
                    String::new()
                } else {
                    raw.to_owned()
                };
                Ok(Self::Grpc { url, service })
            }
            "dns" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Ok(Self::Dns {
                    host: Hostname::new(host)?,
                })
            }
            "file" => {
                let host_remote = url
                    .host_str()
                    .is_some_and(|h| !h.is_empty() && !h.eq_ignore_ascii_case("localhost"));
                if host_remote || url.path().starts_with("//") {
                    return Err(parse_err(
                        input,
                        "remote/UNC file paths are refused (NTLM-relay risk)",
                    ));
                }
                let path = url
                    .to_file_path()
                    .map_err(|()| parse_err(input, "invalid file path"))?;
                if is_unc_or_remote(&path) {
                    return Err(parse_err(
                        input,
                        "remote/UNC file paths are refused (NTLM-relay risk)",
                    ));
                }
                let mode = url.query_pairs().find(|(k, _)| k == "mode").map_or(
                    FileMode::Present,
                    |(_, v)| match v.as_ref() {
                        "absent" => FileMode::Absent,
                        _ => FileMode::Present,
                    },
                );
                Ok(Self::File { path, mode })
            }
            other => Err(Error::UnsupportedScheme(other.into())),
        }
    }
}

fn parse_exec_target(rest: &str, input: &str) -> Result<Target> {
    if rest.is_empty() {
        return Err(parse_err(input, "exec:// requires a program"));
    }
    let (path_part, query_part) = rest.split_once('?').map_or((rest, ""), |(a, b)| (a, b));
    let program_str = percent_decode(path_part)
        .map_err(|e| parse_err(input, &format!("invalid percent-encoding in program: {e}")))?;
    if program_str.is_empty() {
        return Err(parse_err(input, "exec:// program is empty"));
    }
    if program_str.as_bytes().contains(&0) {
        return Err(parse_err(input, "program contains NUL byte"));
    }
    let program_path = PathBuf::from(&program_str);
    let has_sep = program_str.contains('/') || program_str.contains('\\');
    let looks_absolute = program_path.is_absolute()
        || program_str.starts_with('/')
        || program_str
            .as_bytes()
            .get(1)
            .is_some_and(|&b| b == b':' && program_str.as_bytes()[0].is_ascii_alphabetic());
    let is_relative_with_sep = has_sep && !looks_absolute;
    if is_relative_with_sep && std::env::var_os("HOLDON_ALLOW_RELATIVE_EXEC").is_none() {
        return Err(parse_err(
            input,
            "relative exec:// paths resolve against CWD, use an absolute path or set HOLDON_ALLOW_RELATIVE_EXEC=1",
        ));
    }
    let mut args = Vec::new();
    if !query_part.is_empty() {
        for pair in query_part.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k != "arg" {
                return Err(parse_err(
                    input,
                    &format!("unknown exec:// query key `{k}` (only `arg` supported)"),
                ));
            }
            let decoded = percent_decode(v)
                .map_err(|e| parse_err(input, &format!("invalid percent-encoding in arg: {e}")))?;
            if decoded.as_bytes().contains(&0) {
                return Err(parse_err(input, "arg contains NUL byte"));
            }
            args.push(decoded);
        }
    }
    Ok(Target::Exec {
        program: program_path,
        args,
    })
}

fn percent_decode(s: &str) -> std::result::Result<String, &'static str> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err("truncated escape");
            }
            let hi = (bytes[i + 1] as char).to_digit(16).ok_or("bad hex")?;
            let lo = (bytes[i + 2] as char).to_digit(16).ok_or("bad hex")?;
            #[allow(clippy::cast_possible_truncation)]
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "invalid utf-8 after decode")
}

fn parse_port(s: &str, input: &str) -> Result<u16> {
    s.parse::<u16>()
        .map_err(|e| parse_err(input, &format!("bad port: {e}")))
}

fn parse_err(input: &str, reason: &str) -> Error {
    Error::Parse {
        input: input.into(),
        reason: reason.into(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn shorthand_port() {
        let t: Target = ":5432".parse().unwrap();
        assert!(matches!(t, Target::Tcp { ref host, port: 5432 } if host.as_str() == "localhost"));
    }

    #[test]
    fn host_port() {
        let t: Target = "db.local:5432".parse().unwrap();
        assert!(matches!(t, Target::Tcp { ref host, port: 5432 } if host.as_str() == "db.local"));
    }

    #[test]
    fn http_url() {
        let t: Target = "https://api.local/health".parse().unwrap();
        assert!(matches!(t, Target::Http { .. }));
    }

    #[test]
    fn dns_scheme() {
        let t: Target = "dns://example.com".parse().unwrap();
        assert!(matches!(t, Target::Dns { ref host } if host.as_str() == "example.com"));
    }

    #[test]
    fn postgres_url() {
        let t: Target = "postgres://app@db:5432/x".parse().unwrap();
        assert!(matches!(t, Target::Postgres { .. }));
    }

    #[test]
    fn redis_url() {
        let t: Target = "redis://cache:6379".parse().unwrap();
        assert!(matches!(t, Target::Redis { .. }));
    }

    #[test]
    fn password_redacted_in_display() {
        let t: Target = "postgres://app:secret@db:5432/x".parse().unwrap();
        let shown = t.to_string();
        assert!(shown.contains("***"), "got: {shown}");
        assert!(!shown.contains("secret"), "got: {shown}");
    }

    #[test]
    fn password_redacted_in_debug() {
        let t: Target = "postgres://app:secret@db:5432/x".parse().unwrap();
        let shown = format!("{t:?}");
        assert!(!shown.contains("secret"), "leak in Debug: {shown}");
        assert!(shown.contains("***"), "no redaction marker: {shown}");
    }

    #[test]
    fn unc_file_url_rejected() {
        assert!("file://attacker.com/share/x".parse::<Target>().is_err());
    }

    #[test]
    fn mode_absent_substring_no_longer_matches() {
        #[cfg(windows)]
        let url = "file:///C:/?log=mode=absent";
        #[cfg(not(windows))]
        let url = "file:///?log=mode=absent";
        let t: Target = url.parse().unwrap();
        match t {
            Target::File { mode, .. } => assert_eq!(mode, FileMode::Present),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn unsupported_rejected() {
        assert!("ftp://x".parse::<Target>().is_err());
    }

    #[test]
    fn missing_port_rejected() {
        assert!("nohost".parse::<Target>().is_err());
    }

    #[test]
    fn ipv6_bracketed() {
        let t: Target = "[::1]:5432".parse().unwrap();
        assert!(matches!(t, Target::Tcp { ref host, port: 5432 } if host.as_str() == "::1"));
    }

    #[test]
    fn ipv6_unbracketed_rejected() {
        assert!("::1:5432".parse::<Target>().is_err());
    }

    #[test]
    fn exec_relative_path_refused_by_default() {
        assert!("exec://./check.sh".parse::<Target>().is_err());
        assert!("exec://../parent/x".parse::<Target>().is_err());
        assert!("exec://sub/dir/tool".parse::<Target>().is_err());
    }

    #[test]
    fn exec_absolute_path_with_args() {
        let t: Target = "exec:///usr/bin/pg_isready?arg=-h&arg=db".parse().unwrap();
        match t {
            Target::Exec { program, args } => {
                assert_eq!(program, PathBuf::from("/usr/bin/pg_isready"));
                assert_eq!(args, vec!["-h".to_string(), "db".to_string()]);
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn exec_bare_program_resolves_via_path() {
        let t: Target = "exec://pg_isready?arg=-q".parse().unwrap();
        match t {
            Target::Exec { program, args } => {
                assert_eq!(program, PathBuf::from("pg_isready"));
                assert_eq!(args, vec!["-q".to_string()]);
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn exec_empty_program_rejected() {
        assert!("exec://".parse::<Target>().is_err());
        assert!("exec://?arg=x".parse::<Target>().is_err());
    }

    #[test]
    fn exec_percent_decodes_args() {
        let t: Target = "exec://t?arg=hello%20world&arg=a%26b".parse().unwrap();
        match t {
            Target::Exec { args, .. } => {
                assert_eq!(args, vec!["hello world".to_string(), "a&b".to_string()]);
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn exec_unknown_query_key_rejected() {
        assert!("exec://t?cmd=bad".parse::<Target>().is_err());
    }

    #[test]
    fn exec_nul_byte_rejected() {
        assert!("exec://t?arg=a%00b".parse::<Target>().is_err());
        assert!("exec://a%00b".parse::<Target>().is_err());
    }

    #[test]
    fn exec_display_round_trips() {
        let t: Target = "exec://tool?arg=hi&arg=there".parse().unwrap();
        let shown = t.to_string();
        let t2: Target = shown.parse().unwrap();
        match (t, t2) {
            (Target::Exec { args: a1, .. }, Target::Exec { args: a2, .. }) => assert_eq!(a1, a2),
            _ => panic!("round-trip failed"),
        }
    }

    #[test]
    fn tcp_colon_form() {
        let t: Target = "tcp:host:5432".parse().unwrap();
        assert!(matches!(t, Target::Tcp { ref host, port: 5432 } if host.as_str() == "host"));
    }

    #[test]
    fn grpc_plaintext_no_service() {
        let t: Target = "grpc://localhost:50051".parse().unwrap();
        match t {
            Target::Grpc { service, .. } => assert_eq!(service, ""),
            _ => panic!("expected Grpc"),
        }
    }

    #[test]
    fn grpc_tls_with_service() {
        let t: Target = "grpcs://api.example.com:443/my.Service".parse().unwrap();
        match t {
            Target::Grpc { url, service } => {
                assert_eq!(url.scheme(), "grpcs");
                assert_eq!(service, "my.Service");
            }
            _ => panic!("expected Grpc"),
        }
    }

    #[test]
    fn grpc_trailing_slash_normalized() {
        let t: Target = "grpc://localhost:50051/svc/".parse().unwrap();
        match t {
            Target::Grpc { service, .. } => assert_eq!(service, "svc"),
            _ => panic!("expected Grpc"),
        }
    }

    #[test]
    fn grpc_missing_port_rejected() {
        assert!("grpc://localhost".parse::<Target>().is_err());
        assert!("grpcs://api.example.com/svc".parse::<Target>().is_err());
    }
}
