use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use url::Url;

use crate::error::{Error, Result};

const HOSTNAME_MAX_LEN: usize = 253;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Hostname(Box<str>);

impl Hostname {
    pub fn new(s: impl Into<Box<str>>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() {
            return Err(parse_err(&s, "empty hostname"));
        }
        if s.len() > HOSTNAME_MAX_LEN {
            return Err(parse_err(&s, "hostname exceeds 253 bytes"));
        }
        if s.chars().any(char::is_control) {
            return Err(parse_err(&s, "hostname contains control characters"));
        }
        Ok(Self(s))
    }

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

#[derive(Clone)]
#[non_exhaustive]
pub enum Target {
    #[non_exhaustive]
    Tcp {
        host: Hostname,
        port: u16,
        expect: Option<LogMatcher>,
    },
    Http {
        url: Url,
        expect: StatusRange,
    },
    #[non_exhaustive]
    Dns {
        host: Hostname,
        expect_ip: Option<std::net::IpAddr>,
    },
    File {
        path: PathBuf,
        mode: FileMode,
    },
    #[non_exhaustive]
    Postgres {
        url: Url,
        expect_table: Option<String>,
    },
    #[non_exhaustive]
    Redis {
        url: Url,
        expect_key: Option<RedisKeyExpect>,
    },
    #[non_exhaustive]
    Mysql {
        url: Url,
        expect_table: Option<String>,
    },
    Exec {
        program: PathBuf,
        args: Vec<String>,
    },
    Grpc {
        url: Url,
        service: String,
    },
    Log {
        path: PathBuf,
        matcher: LogMatcher,
    },
    Influxdb {
        url: Url,
    },
    Mongodb {
        url: Url,
    },
    Rabbitmq {
        url: Url,
        queue: Option<String>,
        exchange: Option<String>,
    },
    Kafka {
        url: Url,
        topic: Option<String>,
        min_partitions: Option<u32>,
    },
    Temporal {
        url: Url,
    },
    #[non_exhaustive]
    Docker {
        name: String,
        expect: DockerExpect,
    },
    #[non_exhaustive]
    Compose {
        service: String,
        expect: DockerExpect,
    },
    #[non_exhaustive]
    K8s {
        kind: K8sKind,
        namespace: String,
        name: String,
        conditions: Vec<String>,
    },
    Ws {
        url: Url,
        expect: Option<LogMatcher>,
    },
    Process {
        selector: ProcessSelector,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProcessSelector {
    Pid(u32),
    Name(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum K8sKind {
    Pod,
    Deployment,
    Job,
}

impl K8sKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pod => "pod",
            Self::Deployment => "deployment",
            Self::Job => "job",
        }
    }
}

#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct DockerExpect {
    pub state: Option<String>,
    pub require_healthy: bool,
    pub log_match: Option<LogMatcher>,
}

#[derive(Clone)]
#[non_exhaustive]
pub enum LogMatcher {
    Substring(String),
    Regex(std::sync::Arc<regex_lite::Regex>),
}

impl fmt::Debug for LogMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Substring(s) => f.debug_tuple("Substring").field(s).finish(),
            Self::Regex(re) => f.debug_tuple("Regex").field(&re.as_str()).finish(),
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RedisKeyExpect {
    pub key: String,
    pub matcher: Option<LogMatcher>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileMode {
    #[default]
    Present,
    Absent,
}

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusRange {
    lo: u16,
    hi: u16,
}

const HTTP_2XX_LO: u16 = 200;
const HTTP_2XX_HI: u16 = 299;

impl StatusRange {
    #[must_use]
    pub const fn ok_2xx() -> Self {
        Self {
            lo: HTTP_2XX_LO,
            hi: HTTP_2XX_HI,
        }
    }

    #[must_use]
    pub const fn new(lo: u16, hi: u16) -> Self {
        Self {
            lo,
            hi: if hi < lo { lo } else { hi },
        }
    }

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
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp { host, port, expect } => f
                .debug_struct("Tcp")
                .field("host", &host.as_str())
                .field("port", port)
                .field("expect", expect)
                .finish(),
            Self::Http { url, expect } => f
                .debug_struct("Http")
                .field("url", &redact(url))
                .field("expect", expect)
                .finish(),
            Self::Dns { host, expect_ip } => f
                .debug_struct("Dns")
                .field("host", &host.as_str())
                .field("expect_ip", expect_ip)
                .finish(),
            Self::File { path, mode } => f
                .debug_struct("File")
                .field("path", path)
                .field("mode", mode)
                .finish(),
            Self::Postgres { url, expect_table } => f
                .debug_struct("Postgres")
                .field("url", &redact(url))
                .field("expect_table", expect_table)
                .finish(),
            Self::Redis { url, expect_key } => f
                .debug_struct("Redis")
                .field("url", &redact(url))
                .field("expect_key", expect_key)
                .finish(),
            Self::Mysql { url, expect_table } => f
                .debug_struct("Mysql")
                .field("url", &redact(url))
                .field("expect_table", expect_table)
                .finish(),
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
            Self::Log { path, matcher } => f
                .debug_struct("Log")
                .field("path", path)
                .field("matcher", matcher)
                .finish(),
            Self::Influxdb { url } => f
                .debug_struct("Influxdb")
                .field("url", &redact(url))
                .finish(),
            Self::Mongodb { url } => f
                .debug_struct("Mongodb")
                .field("url", &redact(url))
                .finish(),
            Self::Rabbitmq {
                url,
                queue,
                exchange,
            } => f
                .debug_struct("Rabbitmq")
                .field("url", &redact(url))
                .field("queue", queue)
                .field("exchange", exchange)
                .finish(),
            Self::Kafka {
                url,
                topic,
                min_partitions,
            } => f
                .debug_struct("Kafka")
                .field("url", &redact(url))
                .field("topic", topic)
                .field("min_partitions", min_partitions)
                .finish(),
            Self::Temporal { url } => f
                .debug_struct("Temporal")
                .field("url", &redact(url))
                .finish(),
            Self::Docker { name, expect } => f
                .debug_struct("Docker")
                .field("name", name)
                .field("expect", expect)
                .finish(),
            Self::Compose { service, expect } => f
                .debug_struct("Compose")
                .field("service", service)
                .field("expect", expect)
                .finish(),
            Self::K8s {
                kind,
                namespace,
                name,
                conditions,
            } => f
                .debug_struct("K8s")
                .field("kind", &kind.as_str())
                .field("namespace", namespace)
                .field("name", name)
                .field("conditions", conditions)
                .finish(),
            Self::Ws { url, expect } => f
                .debug_struct("Ws")
                .field("url", &redact(url))
                .field("expect", expect)
                .finish(),
            Self::Process { selector } => f
                .debug_struct("Process")
                .field("selector", selector)
                .finish(),
        }
    }
}

impl fmt::Display for Target {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp { host, port, expect } => {
                write!(f, "tcp://{host}:{port}")?;
                match expect {
                    None => Ok(()),
                    Some(LogMatcher::Substring(s)) => {
                        write!(f, "?expect-banner={}", encode_arg(s))
                    }
                    Some(LogMatcher::Regex(re)) => {
                        write!(f, "?expect-banner-regex={}", encode_arg(re.as_str()))
                    }
                }
            }
            Self::Http { url, .. } => write!(f, "{url}"),
            Self::Dns { host, expect_ip } => {
                write!(f, "dns://{host}")?;
                if let Some(ip) = expect_ip {
                    write!(f, "?expect-ip={ip}")?;
                }
                Ok(())
            }
            Self::File { path, mode } => match mode {
                FileMode::Present => write!(f, "file://{}", path.display()),
                FileMode::Absent => write!(f, "file://{}?mode=absent", path.display()),
            },
            Self::Postgres { url, .. }
            | Self::Redis { url, .. }
            | Self::Mysql { url, .. }
            | Self::Grpc { url, .. }
            | Self::Influxdb { url }
            | Self::Mongodb { url }
            | Self::Rabbitmq { url, .. }
            | Self::Kafka { url, .. }
            | Self::Temporal { url } => write!(f, "{}", redact(url)),
            Self::Ws { url, expect } => {
                write!(f, "{}", redact(url))?;
                match expect {
                    None => Ok(()),
                    Some(LogMatcher::Substring(s)) => write!(f, "?expect-text={}", encode_arg(s)),
                    Some(LogMatcher::Regex(re)) => {
                        write!(f, "?expect-regex={}", encode_arg(re.as_str()))
                    }
                }
            }
            Self::Docker { name, expect } => {
                write!(f, "docker://{name}")?;
                write_docker_expect(f, expect)
            }
            Self::Compose { service, expect } => {
                write!(f, "docker-compose://{service}")?;
                write_docker_expect(f, expect)
            }
            Self::K8s {
                kind,
                namespace,
                name,
                conditions,
            } => {
                write!(f, "k8s://{}/{namespace}/{name}", kind.as_str())?;
                if !conditions.is_empty() {
                    write!(f, "?condition={}", conditions.join(","))?;
                }
                Ok(())
            }
            Self::Exec { program, args } => {
                write!(f, "exec://{}", program.display())?;
                for (i, a) in args.iter().enumerate() {
                    let sep = if i == 0 { '?' } else { '&' };
                    write!(f, "{sep}arg={}", encode_arg(a))?;
                }
                Ok(())
            }
            Self::Log { path, matcher } => {
                write!(f, "log://{}", path.display())?;
                match matcher {
                    LogMatcher::Substring(s) => write!(f, "?match={}", encode_arg(s)),
                    LogMatcher::Regex(re) => write!(f, "?regex={}", encode_arg(re.as_str())),
                }
            }
            Self::Process { selector } => match selector {
                ProcessSelector::Pid(pid) => write!(f, "process://{pid}"),
                ProcessSelector::Name(name) => write!(f, "process://{name}"),
            },
        }
    }
}

const MAX_SQL_IDENT_LEN: usize = 63;

fn extract_expect_table(input: &str, url: &Url, scheme: &str) -> Result<Option<String>, Error> {
    let mut found: Option<String> = None;
    for (k, v) in url.query_pairs() {
        if k.as_ref() == "table" {
            if found.is_some() {
                return Err(parse_err(
                    input,
                    &format!("{scheme}:// `?table` may appear at most once"),
                ));
            }
            if v.is_empty() {
                return Err(parse_err(
                    input,
                    &format!("{scheme}:// table name cannot be empty"),
                ));
            }
            validate_sql_identifier(input, &v, scheme)?;
            found = Some(v.into_owned());
        }
    }
    Ok(found)
}

fn validate_sql_identifier(input: &str, name: &str, scheme: &str) -> Result<(), Error> {
    if name.len() > MAX_SQL_IDENT_LEN {
        return Err(parse_err(
            input,
            &format!("{scheme}:// table name exceeds {MAX_SQL_IDENT_LEN} chars"),
        ));
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(parse_err(
            input,
            &format!("{scheme}:// table name cannot be empty"),
        ));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(parse_err(
            input,
            &format!("{scheme}:// table name must start with ASCII letter or underscore"),
        ));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(parse_err(
                input,
                &format!(
                    "{scheme}:// table name has disallowed character `{c}` (use [A-Za-z0-9_])"
                ),
            ));
        }
    }
    Ok(())
}

fn extract_dns_expect_ip(input: &str, url: &Url) -> Result<Option<std::net::IpAddr>> {
    let mut found: Option<std::net::IpAddr> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "expect-ip" => {
                if found.is_some() {
                    return Err(parse_err(
                        input,
                        "dns:// `?expect-ip` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "dns:// `?expect-ip` cannot be empty"));
                }
                let ip: std::net::IpAddr = v.parse().map_err(|_| {
                    parse_err(
                        input,
                        &format!("dns:// `?expect-ip` value `{v}` is not a valid IP address"),
                    )
                })?;
                found = Some(ip);
            }
            other => {
                return Err(parse_err(
                    input,
                    &format!("unknown dns:// query key `{other}` (only `expect-ip` supported)"),
                ));
            }
        }
    }
    Ok(found)
}

fn extract_tcp_expect(input: &str, url: &Url) -> Result<Option<LogMatcher>> {
    let mut needle: Option<String> = None;
    let mut pattern: Option<String> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "expect-banner" => {
                if needle.is_some() {
                    return Err(parse_err(
                        input,
                        "tcp:// `?expect-banner` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "tcp:// `?expect-banner` cannot be empty"));
                }
                needle = Some(v.into_owned());
            }
            "expect-banner-regex" => {
                if pattern.is_some() {
                    return Err(parse_err(
                        input,
                        "tcp:// `?expect-banner-regex` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(
                        input,
                        "tcp:// `?expect-banner-regex` cannot be empty",
                    ));
                }
                pattern = Some(v.into_owned());
            }
            other => {
                return Err(parse_err(
                    input,
                    &format!(
                        "unknown tcp:// query key `{other}` (only `expect-banner` or `expect-banner-regex` supported)"
                    ),
                ));
            }
        }
    }
    match (needle, pattern) {
        (Some(_), Some(_)) => Err(parse_err(
            input,
            "tcp:// `?expect-banner` and `?expect-banner-regex` are mutually exclusive",
        )),
        (Some(s), None) => Ok(Some(LogMatcher::Substring(s))),
        (None, Some(p)) => {
            let re = regex_lite::Regex::new(&p)
                .map_err(|e| parse_err(input, &format!("invalid tcp:// banner regex: {e}")))?;
            Ok(Some(LogMatcher::Regex(std::sync::Arc::new(re))))
        }
        (None, None) => Ok(None),
    }
}

fn extract_ws_expect(input: &str, url: &Url) -> Result<Option<LogMatcher>> {
    let mut needle: Option<String> = None;
    let mut pattern: Option<String> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "expect-text" => {
                if needle.is_some() {
                    return Err(parse_err(
                        input,
                        "ws:// `?expect-text` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "ws:// `?expect-text` cannot be empty"));
                }
                needle = Some(v.into_owned());
            }
            "expect-regex" => {
                if pattern.is_some() {
                    return Err(parse_err(
                        input,
                        "ws:// `?expect-regex` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "ws:// `?expect-regex` cannot be empty"));
                }
                pattern = Some(v.into_owned());
            }
            other => {
                return Err(parse_err(
                    input,
                    &format!(
                        "unknown ws:// query key `{other}` (only `expect-text` or `expect-regex` supported)"
                    ),
                ));
            }
        }
    }
    match (needle, pattern) {
        (Some(_), Some(_)) => Err(parse_err(
            input,
            "ws:// `?expect-text` and `?expect-regex` are mutually exclusive",
        )),
        (Some(s), None) => Ok(Some(LogMatcher::Substring(s))),
        (None, Some(p)) => {
            let re = regex_lite::Regex::new(&p)
                .map_err(|e| parse_err(input, &format!("invalid ws:// regex: {e}")))?;
            Ok(Some(LogMatcher::Regex(std::sync::Arc::new(re))))
        }
        (None, None) => Ok(None),
    }
}

fn drop_ws_expect_query(url: &Url) -> Url {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k.as_ref() != "expect-text" && k.as_ref() != "expect-regex")
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

fn extract_redis_expect(input: &str, url: &Url) -> Result<Option<RedisKeyExpect>> {
    let mut key: Option<String> = None;
    let mut needle: Option<String> = None;
    let mut pattern: Option<String> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "key" => {
                if key.is_some() {
                    return Err(parse_err(input, "redis:// `?key` may appear at most once"));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "redis:// key cannot be empty"));
                }
                key = Some(v.into_owned());
            }
            "match" => {
                if needle.is_some() {
                    return Err(parse_err(
                        input,
                        "redis:// `?match` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "redis:// `?match` cannot be empty"));
                }
                needle = Some(v.into_owned());
            }
            "regex" => {
                if pattern.is_some() {
                    return Err(parse_err(
                        input,
                        "redis:// `?regex` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "redis:// `?regex` cannot be empty"));
                }
                pattern = Some(v.into_owned());
            }
            _ => {}
        }
    }
    let Some(key) = key else {
        if needle.is_some() || pattern.is_some() {
            return Err(parse_err(
                input,
                "redis:// `?match` and `?regex` require `?key`",
            ));
        }
        return Ok(None);
    };
    let matcher = match (needle, pattern) {
        (Some(_), Some(_)) => {
            return Err(parse_err(
                input,
                "redis:// `?match` and `?regex` are mutually exclusive",
            ));
        }
        (Some(s), None) => Some(LogMatcher::Substring(s)),
        (None, Some(p)) => {
            let re = regex_lite::Regex::new(&p)
                .map_err(|e| parse_err(input, &format!("redis:// invalid `?regex`: {e}")))?;
            Some(LogMatcher::Regex(std::sync::Arc::new(re)))
        }
        (None, None) => None,
    };
    Ok(Some(RedisKeyExpect { key, matcher }))
}

fn encode_arg(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' | '=' | '?' | '#' | '%' | ' ' | '+' => {
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

fn local_file_path(url: &Url, input: &str, scheme: &str) -> Result<PathBuf> {
    let host_remote = url
        .host_str()
        .is_some_and(|h| !h.is_empty() && !h.eq_ignore_ascii_case("localhost"));
    if host_remote || url.path().starts_with("//") {
        return Err(parse_err(
            input,
            &format!("remote/UNC {scheme} paths are refused (NTLM-relay risk)"),
        ));
    }
    let path = url
        .to_file_path()
        .map_err(|()| parse_err(input, &format!("invalid {scheme} path")))?;
    if is_unc_or_remote(&path) {
        return Err(parse_err(
            input,
            &format!("remote/UNC {scheme} paths are refused (NTLM-relay risk)"),
        ));
    }
    Ok(path)
}

fn redact(url: &Url) -> String {
    let has_pw = url.password().is_some();
    let has_token = url
        .query_pairs()
        .any(|(k, _)| k.eq_ignore_ascii_case("token"));
    if !has_pw && !has_token {
        return url.to_string();
    }
    let mut clone = url.clone();
    if has_pw {
        let _ = clone.set_password(Some("***"));
    }
    if has_token {
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| {
                if k.eq_ignore_ascii_case("token") {
                    (k.into_owned(), "***".to_owned())
                } else {
                    (k.into_owned(), v.into_owned())
                }
            })
            .collect();
        clone
            .query_pairs_mut()
            .clear()
            .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
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
                    expect: None,
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
                    expect: None,
                });
            }
            if let Some(rest) = input.strip_prefix("tcp:") {
                if !rest.starts_with("//") {
                    let (host, port) = rest
                        .rsplit_once(':')
                        .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                    let port = parse_port(port, input)?;
                    return Ok(Self::Tcp {
                        host: Hostname::new(host)?,
                        port,
                        expect: None,
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
                .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
            let port = parse_port(port, input)?;
            return Ok(Self::Tcp {
                host: Hostname::new(host)?,
                port,
                expect: None,
            });
        }

        let url = Url::parse(input)?;
        match url.scheme() {
            "tcp" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                let port = url
                    .port()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                let expect = extract_tcp_expect(input, &url)?;
                Ok(Self::Tcp {
                    host: Hostname::new(host)?,
                    port,
                    expect,
                })
            }
            "http" | "https" => Ok(Self::Http {
                url,
                expect: StatusRange::default(),
            }),
            "postgres" | "postgresql" => {
                let expect_table = extract_expect_table(input, &url, "postgres")?;
                Ok(Self::Postgres { url, expect_table })
            }
            "redis" | "rediss" => {
                let expect_key = extract_redis_expect(input, &url)?;
                Ok(Self::Redis { url, expect_key })
            }
            "mysql" | "mariadb" => {
                let expect_table = extract_expect_table(input, &url, "mysql")?;
                Ok(Self::Mysql { url, expect_table })
            }
            "mongodb" | "mongodb+srv" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                Ok(Self::Mongodb { url })
            }
            "temporal" | "temporals" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                if url.query().is_some() {
                    return Err(parse_err(
                        input,
                        "temporal:// does not accept query parameters",
                    ));
                }
                Ok(Self::Temporal { url })
            }
            "kafka" | "kafkas" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                let mut topic: Option<String> = None;
                let mut min_partitions: Option<u32> = None;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "topic" => {
                            if v.is_empty() {
                                return Err(parse_err(input, "kafka:// topic cannot be empty"));
                            }
                            topic = Some(v.into_owned());
                        }
                        "expect-partitions" => {
                            let n: u32 = v.parse().map_err(|_| {
                                parse_err(
                                    input,
                                    "kafka:// expect-partitions must be a positive integer",
                                )
                            })?;
                            if n == 0 {
                                return Err(parse_err(
                                    input,
                                    "kafka:// expect-partitions must be at least 1",
                                ));
                            }
                            min_partitions = Some(n);
                        }
                        other => {
                            return Err(parse_err(
                                input,
                                &format!(
                                    "unknown kafka:// query key `{other}` (only `topic` or `expect-partitions` supported)"
                                ),
                            ));
                        }
                    }
                }
                if min_partitions.is_some() && topic.is_none() {
                    return Err(parse_err(
                        input,
                        "kafka:// ?expect-partitions requires ?topic",
                    ));
                }
                Ok(Self::Kafka {
                    url,
                    topic,
                    min_partitions,
                })
            }
            "amqp" | "amqps" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                let mut queue: Option<String> = None;
                let mut exchange: Option<String> = None;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "queue" => {
                            if v.is_empty() {
                                return Err(parse_err(input, "amqp:// queue cannot be empty"));
                            }
                            queue = Some(v.into_owned());
                        }
                        "exchange" => {
                            if v.is_empty() {
                                return Err(parse_err(input, "amqp:// exchange cannot be empty"));
                            }
                            exchange = Some(v.into_owned());
                        }
                        other => {
                            return Err(parse_err(
                                input,
                                &format!(
                                    "unknown amqp:// query key `{other}` (only `queue` or `exchange` supported)"
                                ),
                            ));
                        }
                    }
                }
                Ok(Self::Rabbitmq {
                    url,
                    queue,
                    exchange,
                })
            }
            "influxdb" | "influxdbs" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port_or_known_default()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                for (k, v) in url.query_pairs() {
                    if k.eq_ignore_ascii_case("expect-version") {
                        if v.as_ref() != "1" && v.as_ref() != "2" && v.as_ref() != "3" {
                            return Err(parse_err(
                                input,
                                &format!(
                                    "influxdb:// expect-version `{v}` invalid (only `1`, `2`, or `3`)"
                                ),
                            ));
                        }
                    } else if k.eq_ignore_ascii_case("token") {
                        if v.is_empty() {
                            return Err(parse_err(input, "influxdb:// token cannot be empty"));
                        }
                    } else {
                        return Err(parse_err(
                            input,
                            &format!(
                                "unknown influxdb:// query key `{k}` (only `expect-version` and `token` supported)"
                            ),
                        ));
                    }
                }
                Ok(Self::Influxdb { url })
            }
            "grpc" | "grpcs" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port_or_known_default()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
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
                let expect_ip = extract_dns_expect_ip(input, &url)?;
                Ok(Self::Dns {
                    host: Hostname::new(host)?,
                    expect_ip,
                })
            }
            "file" => {
                let path = local_file_path(&url, input, "file")?;
                let mode = url.query_pairs().find(|(k, _)| k == "mode").map_or(
                    FileMode::Present,
                    |(_, v)| match v.as_ref() {
                        "absent" => FileMode::Absent,
                        _ => FileMode::Present,
                    },
                );
                Ok(Self::File { path, mode })
            }
            "log" => {
                let path = local_file_path(&url, input, "log")?;
                let mut substring: Option<String> = None;
                let mut regex_pat: Option<String> = None;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "match" => substring = Some(v.into_owned()),
                        "regex" => regex_pat = Some(v.into_owned()),
                        other => {
                            return Err(parse_err(
                                input,
                                &format!(
                                    "unknown log:// query key `{other}` (only `match` or `regex` supported)"
                                ),
                            ));
                        }
                    }
                }
                let matcher = match (substring, regex_pat) {
                    (Some(_), Some(_)) => {
                        return Err(parse_err(
                            input,
                            "log:// accepts only one of `match` or `regex`",
                        ));
                    }
                    (Some(s), None) if s.is_empty() => {
                        return Err(parse_err(input, "log:// `match` cannot be empty"));
                    }
                    (None, Some(r)) if r.is_empty() => {
                        return Err(parse_err(input, "log:// `regex` cannot be empty"));
                    }
                    (Some(s), None) => LogMatcher::Substring(s),
                    (None, Some(r)) => {
                        let re = regex_lite::Regex::new(&r)
                            .map_err(|e| parse_err(input, &format!("invalid regex: {e}")))?;
                        LogMatcher::Regex(std::sync::Arc::new(re))
                    }
                    (None, None) => {
                        return Err(parse_err(
                            input,
                            "log:// requires `?match=...` or `?regex=...`",
                        ));
                    }
                };
                Ok(Self::Log { path, matcher })
            }
            "docker" => parse_docker_target(input, &url),
            "docker-compose" => parse_compose_target(input, &url),
            "k8s" | "kubernetes" => parse_k8s_target(input, &url),
            "process" => parse_process_target(input, &url),
            "ws" | "wss" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| parse_err(input, "missing host"))?;
                Hostname::new(host)?;
                url.port_or_known_default()
                    .ok_or_else(|| Error::MissingPort(scrub_target_input(input)))?;
                let expect = extract_ws_expect(input, &url)?;
                let url = drop_ws_expect_query(&url);
                Ok(Self::Ws { url, expect })
            }
            other => Err(Error::UnsupportedScheme(other.into())),
        }
    }
}

fn write_docker_expect(f: &mut fmt::Formatter<'_>, expect: &DockerExpect) -> fmt::Result {
    let mut first = true;
    let mut sep = |f: &mut fmt::Formatter<'_>| -> fmt::Result {
        write!(f, "{}", if first { "?" } else { "&" })?;
        first = false;
        Ok(())
    };
    if let Some(s) = &expect.state {
        sep(f)?;
        write!(f, "state={s}")?;
    }
    if expect.require_healthy {
        sep(f)?;
        write!(f, "healthy=true")?;
    }
    match &expect.log_match {
        Some(LogMatcher::Substring(s)) => {
            sep(f)?;
            write!(f, "log-match={}", encode_arg(s))?;
        }
        Some(LogMatcher::Regex(re)) => {
            sep(f)?;
            write!(f, "log-regex={}", encode_arg(re.as_str()))?;
        }
        None => {}
    }
    Ok(())
}

const DOCKER_STATES: &[&str] = &[
    "created",
    "running",
    "paused",
    "restarting",
    "removing",
    "exited",
    "dead",
];

fn parse_docker_target(input: &str, url: &Url) -> Result<Target> {
    let host = url
        .host_str()
        .ok_or_else(|| parse_err(input, "docker:// requires a container name"))?;
    let path = url.path().trim_matches('/');
    let name = if path.is_empty() {
        host.to_owned()
    } else {
        return Err(parse_err(
            input,
            "docker:// expected docker://<container> (no path segments)",
        ));
    };
    if name.is_empty() {
        return Err(parse_err(input, "docker:// container name is empty"));
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == ' ' || c == '\t')
    {
        return Err(parse_err(
            input,
            "docker:// container name contains illegal characters",
        ));
    }
    let expect = parse_docker_expect_query(input, url, "docker")?;
    Ok(Target::Docker { name, expect })
}

fn parse_compose_target(input: &str, url: &Url) -> Result<Target> {
    let host = url
        .host_str()
        .ok_or_else(|| parse_err(input, "docker-compose:// requires a service name"))?;
    let path = url.path().trim_matches('/');
    if !path.is_empty() {
        return Err(parse_err(
            input,
            "docker-compose:// expected docker-compose://<service> (no path segments)",
        ));
    }
    let service = host.to_owned();
    if service.is_empty() {
        return Err(parse_err(input, "docker-compose:// service name is empty"));
    }
    if service
        .chars()
        .any(|c| c.is_control() || c == ' ' || c == '\t')
    {
        return Err(parse_err(
            input,
            "docker-compose:// service name contains illegal characters",
        ));
    }
    let expect = parse_docker_expect_query(input, url, "docker-compose")?;
    Ok(Target::Compose { service, expect })
}

fn parse_docker_expect_query(input: &str, url: &Url, scheme: &str) -> Result<DockerExpect> {
    let mut expect = DockerExpect::default();
    let mut log_needle: Option<String> = None;
    let mut log_pattern: Option<String> = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "state" => {
                let s = v.to_ascii_lowercase();
                if !DOCKER_STATES.contains(&s.as_str()) {
                    return Err(parse_err(
                        input,
                        &format!(
                            "{scheme}:// state `{s}` invalid (expected one of {DOCKER_STATES:?})"
                        ),
                    ));
                }
                expect.state = Some(s);
            }
            "healthy" => {
                expect.require_healthy = matches!(v.as_ref(), "1" | "true" | "yes");
            }
            "log-match" => {
                if log_needle.is_some() {
                    return Err(parse_err(
                        input,
                        &format!("{scheme}:// `?log-match` may appear at most once"),
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(
                        input,
                        &format!("{scheme}:// `?log-match` cannot be empty"),
                    ));
                }
                log_needle = Some(v.into_owned());
            }
            "log-regex" => {
                if log_pattern.is_some() {
                    return Err(parse_err(
                        input,
                        &format!("{scheme}:// `?log-regex` may appear at most once"),
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(
                        input,
                        &format!("{scheme}:// `?log-regex` cannot be empty"),
                    ));
                }
                log_pattern = Some(v.into_owned());
            }
            other => {
                return Err(parse_err(
                    input,
                    &format!(
                        "unknown {scheme}:// query key `{other}` (only `state`, `healthy`, `log-match`, or `log-regex` supported)"
                    ),
                ));
            }
        }
    }
    expect.log_match = match (log_needle, log_pattern) {
        (Some(_), Some(_)) => {
            return Err(parse_err(
                input,
                &format!("{scheme}:// `?log-match` and `?log-regex` are mutually exclusive"),
            ));
        }
        (Some(s), None) => Some(LogMatcher::Substring(s)),
        (None, Some(p)) => {
            let re = regex_lite::Regex::new(&p)
                .map_err(|e| parse_err(input, &format!("invalid {scheme}:// log-regex: {e}")))?;
            Some(LogMatcher::Regex(std::sync::Arc::new(re)))
        }
        (None, None) => None,
    };
    Ok(expect)
}

fn parse_k8s_target(input: &str, url: &Url) -> Result<Target> {
    let kind_raw = url
        .host_str()
        .ok_or_else(|| parse_err(input, "k8s:// requires <kind>/<namespace>/<name>"))?;
    let kind = match kind_raw.to_ascii_lowercase().as_str() {
        "pod" | "pods" => K8sKind::Pod,
        "deployment" | "deployments" | "deploy" => K8sKind::Deployment,
        "job" | "jobs" => K8sKind::Job,
        other => {
            return Err(parse_err(
                input,
                &format!("k8s:// kind `{other}` not supported (expected pod, deployment, or job)"),
            ));
        }
    };
    let path = url.path().trim_matches('/');
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let [namespace, name] = parts.as_slice() else {
        return Err(parse_err(
            input,
            "k8s:// expected k8s://<kind>/<namespace>/<name>",
        ));
    };
    if !is_valid_k8s_name(namespace) {
        return Err(parse_err(
            input,
            "k8s:// namespace must match RFC 1123 (lowercase, digits, dashes, dots)",
        ));
    }
    if !is_valid_k8s_name(name) {
        return Err(parse_err(
            input,
            "k8s:// resource name must match RFC 1123 (lowercase, digits, dashes, dots)",
        ));
    }
    let mut conditions: Vec<String> = Vec::new();
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "condition" => {
                if !conditions.is_empty() {
                    return Err(parse_err(
                        input,
                        "k8s:// `?condition` may appear at most once",
                    ));
                }
                if v.is_empty() {
                    return Err(parse_err(input, "k8s:// `?condition` cannot be empty"));
                }
                for part in v.split(',') {
                    let trimmed = part.trim();
                    if trimmed.is_empty() {
                        return Err(parse_err(input, "k8s:// `?condition` has an empty entry"));
                    }
                    if !is_valid_condition_type(trimmed) {
                        return Err(parse_err(
                            input,
                            &format!(
                                "k8s:// condition `{trimmed}` must be ASCII alphanumeric (PascalCase)"
                            ),
                        ));
                    }
                    if conditions.iter().any(|c| c.eq_ignore_ascii_case(trimmed)) {
                        return Err(parse_err(
                            input,
                            &format!("k8s:// duplicate condition `{trimmed}`"),
                        ));
                    }
                    conditions.push(trimmed.to_owned());
                }
            }
            other => {
                return Err(parse_err(
                    input,
                    &format!("unknown k8s:// query key `{other}` (only `condition` supported)"),
                ));
            }
        }
    }
    Ok(Target::K8s {
        kind,
        namespace: (*namespace).to_owned(),
        name: (*name).to_owned(),
        conditions,
    })
}

fn is_valid_condition_type(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_valid_k8s_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    let bytes = s.as_bytes();
    let endpoint_ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !endpoint_ok(bytes[0]) || !endpoint_ok(bytes[bytes.len() - 1]) {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

fn parse_process_target(input: &str, url: &Url) -> Result<Target> {
    let host = url
        .host_str()
        .ok_or_else(|| parse_err(input, "process:// requires <pid> or <name>"))?;
    let path = url.path().trim_matches('/');
    if !path.is_empty() {
        return Err(parse_err(
            input,
            "process:// expected process://<pid> or process://<name> (no path segments)",
        ));
    }
    if url.query().is_some() {
        return Err(parse_err(
            input,
            "process:// does not accept query parameters",
        ));
    }
    if host.is_empty() {
        return Err(parse_err(input, "process:// selector is empty"));
    }
    if host.chars().any(char::is_control) {
        return Err(parse_err(
            input,
            "process:// selector contains control characters",
        ));
    }
    let selector = if host.chars().all(|c| c.is_ascii_digit()) {
        let pid: u32 = host
            .parse()
            .map_err(|_| parse_err(input, "process:// pid out of range"))?;
        if pid == 0 {
            return Err(parse_err(input, "process:// pid must be positive"));
        }
        ProcessSelector::Pid(pid)
    } else {
        ProcessSelector::Name(host.to_owned())
    };
    Ok(Target::Process { selector })
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
        input: scrub_target_input(input),
        reason: reason.into(),
    }
}

fn scrub_target_input(input: &str) -> String {
    let scrubbed = scrub_query_secrets(&crate::util::redact_userinfo(input));
    scrubbed
        .chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

fn scrub_query_secrets(input: &str) -> String {
    let Some(q_start) = input.find('?') else {
        return input.to_owned();
    };
    let (head, query) = input.split_at(q_start + 1);
    let scrubbed: Vec<String> = query
        .split('&')
        .map(|pair| {
            if let Some(eq) = pair.find('=') {
                let key = &pair[..eq];
                let decoded_key = percent_decode_lossy(key);
                if decoded_key.eq_ignore_ascii_case("token")
                    || decoded_key.eq_ignore_ascii_case("password")
                {
                    return format!("{key}=***");
                }
            }
            pair.to_owned()
        })
        .collect();
    format!("{head}{}", scrubbed.join("&"))
}

fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hostname_rejects_c0_control() {
        assert!(Hostname::new("evil\x07host").is_err());
    }

    #[test]
    fn hostname_rejects_del() {
        assert!(Hostname::new("evil\x7fhost").is_err());
    }

    #[test]
    fn hostname_rejects_c1_control() {
        assert!(Hostname::new("evil\u{0085}host").is_err());
        assert!(Hostname::new("evil\u{009b}host").is_err());
    }

    #[test]
    fn parse_error_input_strips_control_chars() {
        let err = "evil\u{009b}host:80".parse::<Target>().unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains('\u{009b}'), "raw C1 still in: {msg}");
        assert!(
            msg.contains('\u{fffd}'),
            "expected replacement char in: {msg}"
        );
    }

    #[test]
    fn k8s_pod_parse() {
        let t: Target = "k8s://pod/default/api".parse().unwrap();
        match t {
            Target::K8s {
                kind,
                namespace,
                name,
                conditions,
            } => {
                assert_eq!(kind, K8sKind::Pod);
                assert_eq!(namespace, "default");
                assert_eq!(name, "api");
                assert!(conditions.is_empty());
            }
            _ => panic!("expected K8s variant"),
        }
    }

    #[test]
    fn k8s_condition_single() {
        let t: Target = "k8s://pod/default/api?condition=Ready".parse().unwrap();
        match t {
            Target::K8s { conditions, .. } => assert_eq!(conditions, vec!["Ready".to_owned()]),
            _ => panic!("expected K8s variant"),
        }
    }

    #[test]
    fn k8s_condition_multi() {
        let t: Target = "k8s://pod/default/api?condition=Ready,Initialized,ContainersReady"
            .parse()
            .unwrap();
        match t {
            Target::K8s { conditions, .. } => assert_eq!(
                conditions,
                vec![
                    "Ready".to_owned(),
                    "Initialized".to_owned(),
                    "ContainersReady".to_owned(),
                ]
            ),
            _ => panic!("expected K8s variant"),
        }
    }

    #[test]
    fn k8s_condition_empty_rejected() {
        assert!(
            "k8s://pod/default/api?condition="
                .parse::<Target>()
                .is_err()
        );
        assert!(
            "k8s://pod/default/api?condition=Ready,"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn k8s_condition_invalid_char_rejected() {
        assert!(
            "k8s://pod/default/api?condition=Ready!"
                .parse::<Target>()
                .is_err()
        );
        assert!(
            "k8s://pod/default/api?condition=has-dash"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn k8s_condition_duplicate_value_rejected() {
        assert!(
            "k8s://pod/default/api?condition=Ready,Ready"
                .parse::<Target>()
                .is_err()
        );
        assert!(
            "k8s://pod/default/api?condition=Ready,ready"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn k8s_condition_duplicate_key_rejected() {
        assert!(
            "k8s://pod/default/api?condition=Ready&condition=Initialized"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn k8s_condition_display_roundtrip() {
        let t: Target = "k8s://pod/default/api?condition=Ready,Initialized"
            .parse()
            .unwrap();
        assert_eq!(
            format!("{t}"),
            "k8s://pod/default/api?condition=Ready,Initialized"
        );
    }

    #[test]
    fn k8s_aliases() {
        assert!("k8s://pods/default/api".parse::<Target>().is_ok());
        assert!(
            "k8s://deployments/kube-system/core-dns"
                .parse::<Target>()
                .is_ok()
        );
        assert!(
            "k8s://deploy/kube-system/core-dns"
                .parse::<Target>()
                .is_ok()
        );
        assert!("k8s://jobs/ci/build-42".parse::<Target>().is_ok());
        assert!("kubernetes://pod/default/api".parse::<Target>().is_ok());
    }

    #[test]
    fn k8s_rejects_unknown_kind() {
        assert!("k8s://configmap/default/feature".parse::<Target>().is_err());
    }

    #[test]
    fn k8s_rejects_invalid_name() {
        assert!("k8s://pod/default/CAPS".parse::<Target>().is_err());
        assert!("k8s://pod/default/-leading-dash".parse::<Target>().is_err());
        assert!("k8s://pod//missing-ns".parse::<Target>().is_err());
    }

    #[test]
    fn k8s_rejects_unknown_query() {
        assert!("k8s://pod/default/api?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn k8s_display_roundtrip() {
        let t: Target = "k8s://deployments/kube-system/core-dns".parse().unwrap();
        assert_eq!(format!("{t}"), "k8s://deployment/kube-system/core-dns");
    }

    #[test]
    fn ws_parse_and_redact() {
        let t: Target = "ws://example.com:9000/socket".parse().unwrap();
        assert!(matches!(t, Target::Ws { .. }));
        let t: Target = "wss://example.com/socket".parse().unwrap();
        match &t {
            Target::Ws { url, expect } => {
                assert_eq!(url.port_or_known_default(), Some(443));
                assert!(expect.is_none());
            }
            _ => panic!("expected Ws"),
        }
        let with_pw: Target = "wss://user:hunter2@example.com/socket".parse().unwrap();
        let displayed = format!("{with_pw}");
        assert!(!displayed.contains("hunter2"), "got: {displayed}");
    }

    #[test]
    fn ws_rejects_unknown_query() {
        assert!("ws://host:1/path?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn ws_missing_port_for_plain_ws_is_default_80() {
        assert!("ws://host/p".parse::<Target>().is_ok());
        assert!("wss://host/p".parse::<Target>().is_ok());
    }

    #[test]
    fn ws_expect_text() {
        let t: Target = "ws://host:9000/p?expect-text=hello".parse().unwrap();
        match t {
            Target::Ws { expect, .. } => match expect {
                Some(LogMatcher::Substring(s)) => assert_eq!(s, "hello"),
                _ => panic!("expected Substring matcher"),
            },
            _ => panic!("expected Ws"),
        }
    }

    #[test]
    fn ws_expect_regex() {
        let t: Target = "ws://host:9000/p?expect-regex=%5E%5Cd%2B%24"
            .parse()
            .unwrap();
        match t {
            Target::Ws { expect, .. } => match expect {
                Some(LogMatcher::Regex(re)) => assert_eq!(re.as_str(), "^\\d+$"),
                _ => panic!("expected Regex matcher"),
            },
            _ => panic!("expected Ws"),
        }
    }

    #[test]
    fn ws_expect_text_and_regex_mutually_exclusive() {
        assert!(
            "ws://h:1/p?expect-text=a&expect-regex=b"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn ws_expect_empty_rejected() {
        assert!("ws://h:1/p?expect-text=".parse::<Target>().is_err());
        assert!("ws://h:1/p?expect-regex=".parse::<Target>().is_err());
    }

    #[test]
    fn ws_expect_bad_regex_rejected() {
        assert!("ws://h:1/p?expect-regex=%5B".parse::<Target>().is_err());
    }

    #[test]
    fn ws_display_drops_expect_when_none() {
        let t: Target = "ws://host:9000/p".parse().unwrap();
        let s = format!("{t}");
        assert!(!s.contains("expect"), "got: {s}");
    }

    #[test]
    fn ws_display_includes_expect_text() {
        let t: Target = "ws://host:9000/p?expect-text=hi".parse().unwrap();
        let s = format!("{t}");
        assert!(s.contains("expect-text=hi"), "got: {s}");
    }

    #[test]
    fn docker_scheme_defaults() {
        let t: Target = "docker://api".parse().unwrap();
        match t {
            Target::Docker { name, expect } => {
                assert_eq!(name, "api");
                assert_eq!(expect.state, None);
                assert!(!expect.require_healthy);
            }
            _ => panic!("expected Docker variant"),
        }
    }

    #[test]
    fn docker_scheme_with_state_and_healthy() {
        let t: Target = "docker://api?state=running&healthy=true".parse().unwrap();
        match t {
            Target::Docker { name, expect } => {
                assert_eq!(name, "api");
                assert_eq!(expect.state.as_deref(), Some("running"));
                assert!(expect.require_healthy);
            }
            _ => panic!("expected Docker variant"),
        }
    }

    #[test]
    fn docker_rejects_unknown_state() {
        assert!("docker://api?state=warp".parse::<Target>().is_err());
    }

    #[test]
    fn docker_rejects_unknown_query_key() {
        assert!("docker://api?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn docker_log_match_substring() {
        let t: Target = "docker://api?log-match=Started".parse().unwrap();
        match t {
            Target::Docker { expect, .. } => match expect.log_match {
                Some(LogMatcher::Substring(s)) => assert_eq!(s, "Started"),
                _ => panic!("expected Substring matcher"),
            },
            _ => panic!("expected Docker variant"),
        }
    }

    #[test]
    fn docker_log_regex_compiles() {
        let t: Target = "docker://api?log-regex=ready%5Cs%2Bv%5Cd".parse().unwrap();
        match t {
            Target::Docker { expect, .. } => match expect.log_match {
                Some(LogMatcher::Regex(re)) => assert_eq!(re.as_str(), "ready\\s+v\\d"),
                _ => panic!("expected Regex matcher"),
            },
            _ => panic!("expected Docker variant"),
        }
    }

    #[test]
    fn docker_log_match_and_log_regex_mutually_exclusive() {
        assert!(
            "docker://api?log-match=a&log-regex=b"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn docker_log_match_empty_rejected() {
        assert!("docker://api?log-match=".parse::<Target>().is_err());
        assert!("docker://api?log-regex=".parse::<Target>().is_err());
    }

    #[test]
    fn docker_log_bad_regex_rejected() {
        assert!("docker://api?log-regex=%5B".parse::<Target>().is_err());
    }

    #[test]
    fn docker_display_includes_log_match() {
        let t: Target = "docker://api?log-match=hello".parse().unwrap();
        assert_eq!(format!("{t}"), "docker://api?log-match=hello");
        let t: Target = "docker://api?state=running&log-match=hello"
            .parse()
            .unwrap();
        assert_eq!(format!("{t}"), "docker://api?state=running&log-match=hello");
    }

    #[test]
    fn process_pid() {
        let t: Target = "process://1234".parse().unwrap();
        match t {
            Target::Process {
                selector: ProcessSelector::Pid(p),
            } => assert_eq!(p, 1234),
            _ => panic!("expected Process(Pid)"),
        }
    }

    #[test]
    fn process_name() {
        let t: Target = "process://my-app".parse().unwrap();
        match t {
            Target::Process {
                selector: ProcessSelector::Name(n),
            } => assert_eq!(n, "my-app"),
            _ => panic!("expected Process(Name)"),
        }
    }

    #[test]
    fn process_rejects_zero_pid() {
        assert!("process://0".parse::<Target>().is_err());
    }

    #[test]
    fn process_rejects_query() {
        assert!("process://1234?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn process_rejects_path_segments() {
        assert!("process://name/extra".parse::<Target>().is_err());
    }

    #[test]
    fn process_display_roundtrip() {
        let t: Target = "process://9999".parse().unwrap();
        assert_eq!(format!("{t}"), "process://9999");
        let t: Target = "process://node".parse().unwrap();
        assert_eq!(format!("{t}"), "process://node");
    }

    #[test]
    fn compose_scheme_defaults() {
        let t: Target = "docker-compose://api".parse().unwrap();
        match t {
            Target::Compose { service, expect } => {
                assert_eq!(service, "api");
                assert_eq!(expect.state, None);
                assert!(!expect.require_healthy);
            }
            _ => panic!("expected Compose variant"),
        }
    }

    #[test]
    fn compose_with_expect_query() {
        let t: Target = "docker-compose://api?state=running&healthy=true&log-match=ready"
            .parse()
            .unwrap();
        match t {
            Target::Compose { service, expect } => {
                assert_eq!(service, "api");
                assert_eq!(expect.state.as_deref(), Some("running"));
                assert!(expect.require_healthy);
                assert!(expect.log_match.is_some());
            }
            _ => panic!("expected Compose variant"),
        }
    }

    #[test]
    fn compose_rejects_path_segments() {
        assert!("docker-compose://api/extra".parse::<Target>().is_err());
    }

    #[test]
    fn compose_rejects_unknown_query() {
        assert!("docker-compose://api?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn compose_display_roundtrip() {
        let t: Target = "docker-compose://api".parse().unwrap();
        assert_eq!(format!("{t}"), "docker-compose://api");
        let t: Target = "docker-compose://api?state=exited".parse().unwrap();
        assert_eq!(format!("{t}"), "docker-compose://api?state=exited");
        let t: Target = "docker-compose://api?state=running&healthy=true"
            .parse()
            .unwrap();
        assert_eq!(
            format!("{t}"),
            "docker-compose://api?state=running&healthy=true"
        );
    }

    #[test]
    fn docker_display_roundtrip() {
        let t: Target = "docker://api?state=exited".parse().unwrap();
        assert_eq!(format!("{t}"), "docker://api?state=exited");
        let t: Target = "docker://api?healthy=true".parse().unwrap();
        assert_eq!(format!("{t}"), "docker://api?healthy=true");
        let t: Target = "docker://api?state=running&healthy=true".parse().unwrap();
        assert_eq!(format!("{t}"), "docker://api?state=running&healthy=true");
    }

    #[test]
    fn hostname_accepts_normal_characters() {
        assert!(Hostname::new("db.local").is_ok());
        assert!(Hostname::new("xn--bcher-kva.example").is_ok());
        assert!(Hostname::new("192.168.0.1").is_ok());
    }

    #[test]
    fn shorthand_port() {
        let t: Target = ":5432".parse().unwrap();
        assert!(
            matches!(t, Target::Tcp { ref host, port: 5432, .. } if host.as_str() == "localhost")
        );
    }

    #[test]
    fn tcp_expect_banner_substring() {
        let t: Target = "tcp://host:25?expect-banner=220".parse().unwrap();
        match t {
            Target::Tcp { expect, .. } => match expect {
                Some(LogMatcher::Substring(s)) => assert_eq!(s, "220"),
                _ => panic!("expected Substring matcher"),
            },
            _ => panic!("expected Tcp variant"),
        }
    }

    #[test]
    fn tcp_expect_banner_regex_compiles() {
        let t: Target = "tcp://host:22?expect-banner-regex=%5ESSH-2"
            .parse()
            .unwrap();
        match t {
            Target::Tcp { expect, .. } => match expect {
                Some(LogMatcher::Regex(re)) => assert_eq!(re.as_str(), "^SSH-2"),
                _ => panic!("expected Regex matcher"),
            },
            _ => panic!("expected Tcp variant"),
        }
    }

    #[test]
    fn tcp_expect_banner_mutually_exclusive() {
        assert!(
            "tcp://h:1?expect-banner=a&expect-banner-regex=b"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn tcp_expect_banner_empty_rejected() {
        assert!("tcp://h:1?expect-banner=".parse::<Target>().is_err());
        assert!("tcp://h:1?expect-banner-regex=".parse::<Target>().is_err());
    }

    #[test]
    fn tcp_unknown_query_rejected() {
        assert!("tcp://h:1?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn tcp_display_drops_expect_when_none() {
        let t: Target = "host:5432".parse().unwrap();
        assert_eq!(format!("{t}"), "tcp://host:5432");
    }

    #[test]
    fn tcp_display_includes_expect_banner() {
        let t: Target = "tcp://host:25?expect-banner=220".parse().unwrap();
        assert_eq!(format!("{t}"), "tcp://host:25?expect-banner=220");
    }

    #[test]
    fn host_port() {
        let t: Target = "db.local:5432".parse().unwrap();
        assert!(
            matches!(t, Target::Tcp { ref host, port: 5432, .. } if host.as_str() == "db.local")
        );
    }

    #[test]
    fn http_url() {
        let t: Target = "https://api.local/health".parse().unwrap();
        assert!(matches!(t, Target::Http { .. }));
    }

    #[test]
    fn dns_scheme() {
        let t: Target = "dns://example.com".parse().unwrap();
        assert!(matches!(t, Target::Dns { ref host, .. } if host.as_str() == "example.com"));
    }

    #[test]
    fn dns_expect_ip_v4() {
        let t: Target = "dns://example.com?expect-ip=10.0.0.1".parse().unwrap();
        match t {
            Target::Dns { expect_ip, .. } => assert_eq!(
                expect_ip,
                Some("10.0.0.1".parse::<std::net::IpAddr>().unwrap())
            ),
            _ => panic!("expected Dns variant"),
        }
    }

    #[test]
    fn dns_expect_ip_v6() {
        let t: Target = "dns://example.com?expect-ip=2001:db8::1".parse().unwrap();
        match t {
            Target::Dns { expect_ip, .. } => assert_eq!(
                expect_ip,
                Some("2001:db8::1".parse::<std::net::IpAddr>().unwrap())
            ),
            _ => panic!("expected Dns variant"),
        }
    }

    #[test]
    fn dns_expect_ip_invalid_rejected() {
        assert!(
            "dns://example.com?expect-ip=not-an-ip"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn dns_expect_ip_empty_rejected() {
        assert!("dns://example.com?expect-ip=".parse::<Target>().is_err());
    }

    #[test]
    fn dns_expect_ip_duplicate_rejected() {
        assert!(
            "dns://example.com?expect-ip=1.1.1.1&expect-ip=2.2.2.2"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn dns_unknown_query_rejected() {
        assert!("dns://example.com?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn dns_display_drops_expect_when_none() {
        let t: Target = "dns://example.com".parse().unwrap();
        assert_eq!(format!("{t}"), "dns://example.com");
    }

    #[test]
    fn dns_display_includes_expect_ip() {
        let t: Target = "dns://example.com?expect-ip=1.2.3.4".parse().unwrap();
        assert_eq!(format!("{t}"), "dns://example.com?expect-ip=1.2.3.4");
    }

    #[test]
    fn dns_display_roundtrip_ipv6() {
        let t: Target = "dns://example.com?expect-ip=2001:db8::1".parse().unwrap();
        assert_eq!(format!("{t}"), "dns://example.com?expect-ip=2001:db8::1");
    }

    #[test]
    fn postgres_url() {
        let t: Target = "postgres://app@db:5432/x".parse().unwrap();
        assert!(matches!(
            t,
            Target::Postgres {
                expect_table: None,
                ..
            }
        ));
    }

    #[test]
    fn postgres_expect_table_extracted() {
        let t: Target = "postgres://app@db:5432/x?table=users".parse().unwrap();
        match t {
            Target::Postgres { expect_table, .. } => {
                assert_eq!(expect_table.as_deref(), Some("users"));
            }
            _ => panic!("expected Postgres variant"),
        }
    }

    #[test]
    fn mysql_expect_table_extracted() {
        let t: Target = "mysql://app@db:3306/x?table=orders".parse().unwrap();
        match t {
            Target::Mysql { expect_table, .. } => {
                assert_eq!(expect_table.as_deref(), Some("orders"));
            }
            _ => panic!("expected Mysql variant"),
        }
    }

    #[test]
    fn expect_table_rejects_invalid_identifier() {
        assert!(
            "postgres://app@db/x?table=users;DROP"
                .parse::<Target>()
                .is_err()
        );
        assert!(
            "postgres://app@db/x?table=1users"
                .parse::<Target>()
                .is_err()
        );
        assert!("postgres://app@db/x?table=".parse::<Target>().is_err());
        assert!("mysql://app@db/x?table=a-b".parse::<Target>().is_err());
    }

    #[test]
    fn expect_table_accepts_underscored_identifier() {
        assert!(
            "postgres://app@db/x?table=user_accounts"
                .parse::<Target>()
                .is_ok()
        );
        assert!("mysql://app@db/x?table=_internal".parse::<Target>().is_ok());
    }

    #[test]
    fn expect_table_rejects_overlong_name() {
        let long = "a".repeat(64);
        let input = format!("postgres://app@db/x?table={long}");
        assert!(input.parse::<Target>().is_err());
    }

    #[test]
    fn expect_table_round_trips_in_display() {
        let t: Target = "postgres://app@db:5432/x?table=users".parse().unwrap();
        let shown = t.to_string();
        assert!(shown.contains("table=users"), "got: {shown}");
    }

    #[test]
    fn redis_url() {
        let t: Target = "redis://cache:6379".parse().unwrap();
        assert!(matches!(
            t,
            Target::Redis {
                expect_key: None,
                ..
            }
        ));
    }

    #[test]
    fn redis_expect_key_only() {
        let t: Target = "redis://cache:6379?key=ready".parse().unwrap();
        match t {
            Target::Redis {
                expect_key: Some(e),
                ..
            } => {
                assert_eq!(e.key, "ready");
                assert!(e.matcher.is_none());
            }
            _ => panic!("expected Redis with key"),
        }
    }

    #[test]
    fn redis_expect_key_with_substring_match() {
        let t: Target = "redis://cache:6379?key=status&match=UP".parse().unwrap();
        match t {
            Target::Redis {
                expect_key: Some(e),
                ..
            } => {
                assert_eq!(e.key, "status");
                assert!(matches!(e.matcher, Some(LogMatcher::Substring(ref s)) if s == "UP"));
            }
            _ => panic!("expected Redis with key+match"),
        }
    }

    #[test]
    fn redis_expect_key_with_regex() {
        let t: Target = "redis://cache:6379?key=health&regex=^(ok|UP)$"
            .parse()
            .unwrap();
        match t {
            Target::Redis {
                expect_key: Some(e),
                ..
            } => {
                assert_eq!(e.key, "health");
                assert!(matches!(e.matcher, Some(LogMatcher::Regex(_))));
            }
            _ => panic!("expected Redis with key+regex"),
        }
    }

    #[test]
    fn redis_match_without_key_rejected() {
        assert!("redis://cache?match=foo".parse::<Target>().is_err());
        assert!("redis://cache?regex=foo".parse::<Target>().is_err());
    }

    #[test]
    fn redis_match_and_regex_mutually_exclusive() {
        assert!(
            "redis://cache?key=k&match=a&regex=b"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn redis_empty_key_rejected() {
        assert!("redis://cache?key=".parse::<Target>().is_err());
    }

    #[test]
    fn redis_empty_match_or_regex_rejected() {
        assert!("redis://cache?key=k&match=".parse::<Target>().is_err());
        assert!("redis://cache?key=k&regex=".parse::<Target>().is_err());
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
        assert!(matches!(t, Target::Tcp { ref host, port: 5432, .. } if host.as_str() == "::1"));
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
        assert!(matches!(t, Target::Tcp { ref host, port: 5432, .. } if host.as_str() == "host"));
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

    #[cfg(unix)]
    #[test]
    fn log_substring_match_parses() {
        let t: Target = "log:///tmp/app.log?match=Listening".parse().unwrap();
        match t {
            Target::Log { path, matcher } => {
                assert_eq!(path, PathBuf::from("/tmp/app.log"));
                assert!(matches!(matcher, LogMatcher::Substring(ref s) if s == "Listening"));
            }
            _ => panic!("expected Log"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn log_regex_compiles_at_parse() {
        let t: Target = "log:///tmp/app.log?regex=Listening%20on%20%5Cd%2B"
            .parse()
            .unwrap();
        match t {
            Target::Log {
                matcher: LogMatcher::Regex(re),
                ..
            } => {
                assert!(re.is_match("Listening on 8080"));
            }
            _ => panic!("expected Log/Regex"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn log_requires_matcher() {
        assert!("log:///tmp/app.log".parse::<Target>().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn log_rejects_both_match_and_regex() {
        assert!(
            "log:///tmp/app.log?match=x&regex=y"
                .parse::<Target>()
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn log_rejects_unknown_query() {
        assert!(
            "log:///tmp/app.log?from=end&match=x"
                .parse::<Target>()
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn log_rejects_remote_host() {
        assert!(
            "log://attacker.com/tmp/app.log?match=x"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn influxdb_plain_parses() {
        let t: Target = "influxdb://localhost:8086".parse().unwrap();
        assert!(matches!(t, Target::Influxdb { .. }));
    }

    #[test]
    fn influxdb_tls_parses() {
        let t: Target = "influxdbs://h:8086".parse().unwrap();
        assert!(matches!(t, Target::Influxdb { .. }));
    }

    #[test]
    fn influxdb_with_valid_version_parses() {
        let t: Target = "influxdb://h:8086?expect-version=2".parse().unwrap();
        assert!(matches!(t, Target::Influxdb { .. }));
    }

    #[test]
    fn influxdb_rejects_bad_version_at_parse() {
        assert!(
            "influxdb://h:8086?expect-version=4"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn influxdb_accepts_version_3_at_parse() {
        let t: Target = "influxdb://h:8086?expect-version=3".parse().unwrap();
        assert!(matches!(t, Target::Influxdb { .. }));
    }

    #[test]
    fn influxdb_accepts_token_at_parse() {
        let t: Target = "influxdb://h:8086?token=secret".parse().unwrap();
        assert!(matches!(t, Target::Influxdb { .. }));
    }

    #[test]
    fn influxdb_rejects_empty_token() {
        assert!("influxdb://h:8086?token=".parse::<Target>().is_err());
    }

    #[test]
    fn influxdb_display_redacts_token() {
        let t: Target = "influxdb://h:8086?token=apiv3_supersecret".parse().unwrap();
        let s = format!("{t}");
        assert!(!s.contains("apiv3_supersecret"));
        assert!(s.contains("token=***"));
    }

    #[test]
    fn influxdb_display_keeps_other_query_params() {
        let t: Target = "influxdb://h:8086?token=secret&expect-version=3"
            .parse()
            .unwrap();
        let s = format!("{t}");
        assert!(!s.contains("secret"));
        assert!(s.contains("expect-version=3"));
    }

    #[test]
    fn influxdb_parse_error_scrubs_token() {
        let err = "influxdb://h:8086?token=verysecret&expect-version=4"
            .parse::<Target>()
            .unwrap_err()
            .to_string();
        assert!(!err.contains("verysecret"));
        assert!(err.contains("token=***"));
    }

    #[test]
    fn parse_err_scrubs_percent_encoded_token_key() {
        let err = "influxdb://h:8086?tok%65n=verysecret&expect-version=4"
            .parse::<Target>()
            .unwrap_err()
            .to_string();
        assert!(!err.contains("verysecret"));
        assert!(err.contains("=***"));
    }

    #[test]
    fn influxdb_rejects_unknown_query_at_parse() {
        assert!(
            "influxdb://h:8086?bucket=metrics"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn influxdb_missing_port_rejected() {
        assert!("influxdb://localhost".parse::<Target>().is_err());
    }

    #[test]
    fn mongodb_plain_parses() {
        let t: Target = "mongodb://localhost:27017".parse().unwrap();
        assert!(matches!(t, Target::Mongodb { .. }));
    }

    #[test]
    fn mongodb_with_userinfo_parses() {
        let t: Target = "mongodb://user:pass@localhost:27017/admin".parse().unwrap();
        assert!(matches!(t, Target::Mongodb { .. }));
    }

    #[test]
    fn mongodb_srv_parses() {
        let t: Target = "mongodb+srv://cluster.example.com/db".parse().unwrap();
        assert!(matches!(t, Target::Mongodb { .. }));
    }

    #[test]
    fn mongodb_display_redacts_password() {
        let t: Target = "mongodb://user:secret@h:27017".parse().unwrap();
        let s = format!("{t}");
        assert!(!s.contains("secret"));
        assert!(s.contains("***"));
    }

    #[test]
    fn amqp_plain_parses() {
        let t: Target = "amqp://localhost:5672".parse().unwrap();
        match t {
            Target::Rabbitmq {
                queue, exchange, ..
            } => {
                assert!(queue.is_none());
                assert!(exchange.is_none());
            }
            _ => panic!("expected Rabbitmq"),
        }
    }

    #[test]
    fn amqps_with_vhost_parses() {
        let t: Target = "amqps://u:p@h:5671/myvhost".parse().unwrap();
        assert!(matches!(t, Target::Rabbitmq { .. }));
    }

    #[test]
    fn amqp_extracts_queue_and_exchange() {
        let t: Target = "amqp://h:5672/?queue=jobs&exchange=events".parse().unwrap();
        match t {
            Target::Rabbitmq {
                queue, exchange, ..
            } => {
                assert_eq!(queue.as_deref(), Some("jobs"));
                assert_eq!(exchange.as_deref(), Some("events"));
            }
            _ => panic!("expected Rabbitmq"),
        }
    }

    #[test]
    fn amqp_rejects_unknown_query() {
        assert!("amqp://h:5672/?foo=bar".parse::<Target>().is_err());
    }

    #[test]
    fn amqp_rejects_empty_queue() {
        assert!("amqp://h:5672/?queue=".parse::<Target>().is_err());
    }

    #[test]
    fn amqp_display_redacts_password() {
        let t: Target = "amqp://user:secret@h:5672".parse().unwrap();
        let s = format!("{t}");
        assert!(!s.contains("secret"));
        assert!(s.contains("***"));
    }

    #[test]
    fn kafka_plain_parses() {
        let t: Target = "kafka://broker:9092".parse().unwrap();
        match t {
            Target::Kafka {
                topic,
                min_partitions,
                ..
            } => {
                assert!(topic.is_none());
                assert!(min_partitions.is_none());
            }
            _ => panic!("expected Kafka"),
        }
    }

    #[test]
    fn kafka_tls_parses() {
        let t: Target = "kafkas://broker:9093".parse().unwrap();
        assert!(matches!(t, Target::Kafka { .. }));
    }

    #[test]
    fn kafka_topic_and_partitions_parses() {
        let t: Target = "kafka://h:9092/?topic=orders&expect-partitions=12"
            .parse()
            .unwrap();
        match t {
            Target::Kafka {
                topic,
                min_partitions,
                ..
            } => {
                assert_eq!(topic.as_deref(), Some("orders"));
                assert_eq!(min_partitions, Some(12));
            }
            _ => panic!("expected Kafka"),
        }
    }

    #[test]
    fn kafka_rejects_partitions_without_topic() {
        assert!(
            "kafka://h:9092/?expect-partitions=3"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn kafka_rejects_zero_partitions() {
        assert!(
            "kafka://h:9092/?topic=t&expect-partitions=0"
                .parse::<Target>()
                .is_err()
        );
    }

    #[test]
    fn kafka_rejects_unknown_query_key() {
        assert!("kafka://h:9092/?group=x".parse::<Target>().is_err());
    }

    #[test]
    fn kafka_missing_port_rejected() {
        assert!("kafka://broker".parse::<Target>().is_err());
    }

    #[test]
    fn temporal_plain_parses() {
        let t: Target = "temporal://localhost:7233".parse().unwrap();
        assert!(matches!(t, Target::Temporal { .. }));
    }

    #[test]
    fn temporal_tls_parses() {
        let t: Target = "temporals://cloud.temporal.io:7233".parse().unwrap();
        assert!(matches!(t, Target::Temporal { .. }));
    }

    #[test]
    fn temporal_missing_port_rejected() {
        assert!("temporal://localhost".parse::<Target>().is_err());
    }

    #[test]
    fn temporal_rejects_query_params() {
        assert!("temporal://h:7233/?mode=worker".parse::<Target>().is_err());
    }
}
