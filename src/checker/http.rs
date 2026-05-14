use std::sync::OnceLock;
use std::time::Instant;

pub use reqwest::Method;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::fmt::Write as _;

use reqwest::redirect::Policy;
use reqwest::tls::Version as TlsVersion;
use reqwest::{Certificate, Client};
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::StatusRange;
use crate::util::{format_error_chain, redact_in};

/// Maximum bytes of response body read when matching `body_substring`. Bodies
/// larger than this are truncated. Healthchecks rarely return more than a few
/// KiB so a 1 MiB ceiling is generous and bounds memory.
const MAX_BODY_BYTES: u64 = 1_024 * 1_024;
/// Maximum bytes of response body kept for the diagnostic snippet shown on
/// status mismatch. Small enough to keep stage messages readable.
const FAILURE_BODY_SNIPPET_BYTES: usize = 240;
/// Response headers whose values are shown verbatim on failure to help
/// identify the upstream server.
const SERVER_HINT_HEADERS: &[&str] = &["server", "x-powered-by", "via"];
/// Maximum visible characters for a single upstream-identification header
/// value. Long `Via:` chains or verbose `Server:` strings get truncated.
const SERVER_HINT_VALUE_MAX: usize = 80;

/// Minimum TLS protocol version accepted by HTTPS probes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TlsMin {
    /// TLS 1.2 (current default per IETF deprecation of 1.0/1.1).
    #[default]
    V12,
    /// TLS 1.3.
    V13,
}

impl TlsMin {
    const fn into_reqwest(self) -> TlsVersion {
        match self {
            Self::V12 => TlsVersion::TLS_1_2,
            Self::V13 => TlsVersion::TLS_1_3,
        }
    }
}

/// Process-wide HTTP request configuration applied to every `http(s)://` probe.
///
/// Set once from the CLI layer via [`set_global`] before the first probe.
/// Library users that need per-target overrides can leave this unset and use
/// the defaults.
#[derive(Debug, Default, Clone)]
pub struct HttpConfig {
    /// Extra request headers, applied to every HTTP probe.
    pub headers: HeaderMap,
    /// HTTP method. Defaults to `GET`.
    pub method: Method,
    /// When true, TLS certificate verification is disabled. Use only for
    /// self-signed development endpoints.
    pub insecure: bool,
    /// If true (default), follow up to 5 redirects but refuse `https → http`.
    /// If false, the first response (any 3xx) is reported as-is.
    pub follow_redirects: bool,
    /// Substring that must appear in the response body for the probe to pass.
    /// `None` skips body inspection. Body is capped at 1 MiB.
    pub body_substring: Option<String>,
    /// Compiled regular expression the response body must match.
    pub body_regex: Option<regex_lite::Regex>,
    /// RFC 6901 JSON pointer + expected string value (`pointer=value`). On
    /// `/status=UP`, holdon parses the body as JSON and checks that the value
    /// at `/status` equals `"UP"`. Numbers and booleans are compared via
    /// their JSON `to_string` (`true`, `42.5`).
    pub body_json_match: Option<(String, String)>,
    /// Custom CA certificates in PEM, appended to the bundled webpki roots.
    pub extra_ca_pem: Vec<Vec<u8>>,
    /// Minimum TLS protocol version. Defaults to TLS 1.2.
    pub min_tls: TlsMin,
    /// Optional request body. Sent verbatim with `Content-Type:
    /// application/octet-stream` unless the caller supplies a `Content-Type`
    /// header via [`HttpConfig::headers`].
    pub body: Option<Vec<u8>>,
    /// Client identity for mutual TLS, as a concatenated PEM bundle containing
    /// the certificate chain followed by the private key. Parsed once when the
    /// shared HTTP client is built. Invalid PEM is reported on stderr and the
    /// probe runs without client auth.
    pub client_identity_pem: Option<Vec<u8>>,
    /// Response header assertions. Each pair is a header name plus a regex the
    /// header value must match. A missing header is treated as a failure with
    /// the `HTTP_HEADER_MISSING` hint. Repeating a name applies an `AND` of
    /// regexes against the same header.
    pub header_expectations: Vec<(HeaderName, regex_lite::Regex)>,
}

impl HttpConfig {
    /// Builds a default config with `follow_redirects = true`. Workarounds for
    /// `#[derive(Default)]` setting bool to `false`.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            follow_redirects: true,
            ..Self::default()
        }
    }
}

static CONFIG: OnceLock<HttpConfig> = OnceLock::new();
static CLIENT: OnceLock<Client> = OnceLock::new();

/// Installs the process-wide HTTP probe configuration.
///
/// First call wins. Subsequent calls are silently ignored.
pub fn set_global(cfg: HttpConfig) {
    let _ = CONFIG.set(cfg);
}

fn config() -> &'static HttpConfig {
    CONFIG.get_or_init(HttpConfig::defaults)
}

#[cfg(feature = "influxdb")]
pub(crate) fn raw_client() -> &'static Client {
    client()
}

fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        let cfg = config();
        let policy = if cfg.follow_redirects {
            Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("too many redirects");
                }
                let prev_was_https = attempt
                    .previous()
                    .last()
                    .is_some_and(|u| u.scheme() == "https");
                if prev_was_https && attempt.url().scheme() != "https" {
                    return attempt.error("refusing https to http downgrade");
                }
                attempt.follow()
            })
        } else {
            Policy::none()
        };
        let mut b = Client::builder()
            .user_agent(concat!("holdon/", env!("CARGO_PKG_VERSION")))
            .redirect(policy)
            .min_tls_version(cfg.min_tls.into_reqwest());
        if cfg.insecure {
            b = b.danger_accept_invalid_certs(true);
        }
        for pem in &cfg.extra_ca_pem {
            match Certificate::from_pem_bundle(pem) {
                Ok(certs) if certs.is_empty() => {
                    eprintln!("holdon: --ca-cert bundle contained no certificates");
                }
                Ok(certs) => {
                    for cert in certs {
                        b = b.add_root_certificate(cert);
                    }
                }
                Err(e) => {
                    eprintln!("holdon: failed to parse --ca-cert bundle: {e}");
                }
            }
        }
        if let Some(pem) = cfg.client_identity_pem.as_deref() {
            match reqwest::Identity::from_pem(pem) {
                Ok(id) => b = b.identity(id),
                Err(e) => {
                    eprintln!("holdon: failed to load --client-cert/--client-key: {e}");
                }
            }
        }
        b.build().unwrap_or_else(|_| Client::new())
    })
}

pub(super) async fn probe(url: &Url, expect: &StatusRange, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let cfg = config();
    let mut req = client().request(cfg.method.clone(), url.clone());
    if !cfg.headers.is_empty() {
        req = req.headers(cfg.headers.clone());
    }
    if let Some(body) = &cfg.body {
        req = req.body(body.clone());
        if !cfg.headers.contains_key(reqwest::header::CONTENT_TYPE) {
            req = req.header(reqwest::header::CONTENT_TYPE, "application/octet-stream");
        }
    }
    let stage = match req.timeout(ctx.attempt_timeout).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if !expect.contains(status) {
                let server_tag = upstream_hint(resp.headers());
                let snippet = read_body_snippet(resp).await;
                let mut msg = format!("status {status}");
                if let Some(tag) = server_tag {
                    let _ = write!(msg, " [{tag}]");
                }
                if !snippet.is_empty() {
                    msg.push_str(": ");
                    msg.push_str(&snippet);
                }
                if !pw.is_empty() {
                    msg = redact_in(&msg, &pw);
                }
                err_stage(
                    StageKind::Http,
                    start.elapsed(),
                    msg,
                    Some(hints::HTTP_RETRY),
                )
            } else if let Some(header_err) =
                evaluate_header_expectations(cfg, resp.headers(), start)
            {
                header_err
            } else if needs_body_inspection(cfg) {
                match read_body_capped(resp).await {
                    Ok(body) => evaluate_body_matchers(cfg, &body, start),
                    Err(e) => {
                        let hint = e.hint();
                        let mut msg = format_error_chain(&e);
                        if !pw.is_empty() {
                            msg = redact_in(&msg, &pw);
                        }
                        err_stage(StageKind::Http, start.elapsed(), msg, hint)
                    }
                }
            } else {
                ok_stage(StageKind::Http, start.elapsed())
            }
        }
        Err(e) if e.is_timeout() => err_stage(
            StageKind::Http,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::SERVER_SLOW),
        ),
        Err(e) => {
            let hint = e.hint();
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Http, start.elapsed(), msg, hint)
        }
    };
    vec![stage]
}

const fn needs_body_inspection(cfg: &HttpConfig) -> bool {
    cfg.body_substring.is_some() || cfg.body_regex.is_some() || cfg.body_json_match.is_some()
}

fn evaluate_header_expectations(
    cfg: &HttpConfig,
    headers: &HeaderMap,
    start: Instant,
) -> Option<Stage> {
    for (name, pattern) in &cfg.header_expectations {
        let Some(raw) = headers.get(name) else {
            return Some(err_stage(
                StageKind::Http,
                start.elapsed(),
                format!("expected header `{}` not present", name.as_str()),
                Some(hints::HTTP_HEADER_MISSING),
            ));
        };
        let Ok(value) = raw.to_str() else {
            return Some(err_stage(
                StageKind::Http,
                start.elapsed(),
                format!("header `{}` contained non-ascii bytes", name.as_str()),
                Some(hints::HTTP_HEADER_ENCODING),
            ));
        };
        if !pattern.is_match(value) {
            return Some(err_stage(
                StageKind::Http,
                start.elapsed(),
                format!(
                    "header `{}` was `{value}`, did not match regex `{}`",
                    name.as_str(),
                    pattern.as_str()
                ),
                Some(hints::HTTP_HEADER_MISMATCH),
            ));
        }
    }
    None
}

fn evaluate_body_matchers(cfg: &HttpConfig, body: &str, start: Instant) -> Stage {
    if let Some(needle) = cfg.body_substring.as_deref() {
        if !body.contains(needle) {
            return err_stage(
                StageKind::Http,
                start.elapsed(),
                "body did not contain expected substring",
                Some(hints::HTTP_BODY_MISMATCH),
            );
        }
    }
    if let Some(re) = cfg.body_regex.as_ref() {
        if !re.is_match(body) {
            return err_stage(
                StageKind::Http,
                start.elapsed(),
                format!("body did not match regex `{}`", re.as_str()),
                Some(hints::HTTP_BODY_REGEX_MISMATCH),
            );
        }
    }
    if let Some((pointer, expected)) = cfg.body_json_match.as_ref() {
        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(value) => match value.pointer(pointer) {
                Some(found) if json_value_matches(found, expected) => {}
                Some(found) => {
                    return err_stage(
                        StageKind::Http,
                        start.elapsed(),
                        format!(
                            "json pointer `{pointer}` was `{}`, expected `{expected}`",
                            display_json_value(found)
                        ),
                        Some(hints::HTTP_JSON_MISMATCH),
                    );
                }
                None => {
                    return err_stage(
                        StageKind::Http,
                        start.elapsed(),
                        format!("json pointer `{pointer}` not present in body"),
                        Some(hints::HTTP_JSON_MISMATCH),
                    );
                }
            },
            Err(e) => {
                return err_stage(
                    StageKind::Http,
                    start.elapsed(),
                    format!("response body is not valid JSON: {e}"),
                    Some(hints::HTTP_JSON_MISMATCH),
                );
            }
        }
    }
    ok_stage(StageKind::Http, start.elapsed())
}

fn json_value_matches(found: &serde_json::Value, expected: &str) -> bool {
    match found {
        serde_json::Value::String(s) => s == expected,
        serde_json::Value::Bool(b) => b.to_string() == expected,
        serde_json::Value::Number(n) => n.to_string() == expected,
        serde_json::Value::Null => expected == "null",
        _ => false,
    }
}

fn display_json_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn upstream_hint(headers: &HeaderMap) -> Option<String> {
    let mut parts = Vec::new();
    for name in SERVER_HINT_HEADERS {
        if let Some(value) = headers.get(*name).and_then(|v| v.to_str().ok()) {
            let cleaned = crate::util::sanitize_for_terminal(value);
            let trimmed = cleaned.trim();
            if trimmed.is_empty() {
                continue;
            }
            let bounded = if trimmed.chars().count() > SERVER_HINT_VALUE_MAX {
                let mut t: String = trimmed.chars().take(SERVER_HINT_VALUE_MAX).collect();
                t.push('…');
                t
            } else {
                trimmed.to_owned()
            };
            parts.push(format!("{name}: {bounded}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

async fn read_body_snippet(resp: reqwest::Response) -> String {
    let raw = read_body_to(resp, FAILURE_BODY_SNIPPET_BYTES * 4)
        .await
        .unwrap_or_default();
    if raw.is_empty() {
        return String::new();
    }
    let cleaned = crate::util::sanitize_for_terminal(&raw);
    let mut compact = String::with_capacity(cleaned.len());
    let mut prev_ws = false;
    for ch in cleaned.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                compact.push(' ');
            }
            prev_ws = true;
        } else {
            compact.push(ch);
            prev_ws = false;
        }
    }
    let compact = compact.trim();
    if compact.chars().count() <= FAILURE_BODY_SNIPPET_BYTES {
        return compact.to_owned();
    }
    let mut out: String = compact.chars().take(FAILURE_BODY_SNIPPET_BYTES).collect();
    out.push('…');
    out
}

async fn read_body_capped(resp: reqwest::Response) -> reqwest::Result<String> {
    read_body_to(resp, usize::try_from(MAX_BODY_BYTES).unwrap_or(usize::MAX)).await
}

async fn read_body_to(mut resp: reqwest::Response, cap: usize) -> reqwest::Result<String> {
    let mut buf = Vec::with_capacity(4096);
    while let Some(bytes) = resp.chunk().await? {
        let remaining = cap.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        let take = bytes.len().min(remaining);
        buf.extend_from_slice(&bytes[..take]);
        if take < bytes.len() {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Parses a single `Name: Value` header string into a typed pair.
///
/// Whitespace around the colon and value is trimmed. Both halves are validated
/// against the HTTP grammar. Control bytes and invalid characters are rejected.
///
/// # Errors
/// Returns a human-readable message when the input is missing a colon, has an
/// empty name, or contains characters disallowed by RFC 7230.
pub fn parse_header(input: &str) -> Result<(HeaderName, HeaderValue), String> {
    let (name, value) = input
        .split_once(':')
        .ok_or_else(|| format!("missing `:` in header `{input}`"))?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() {
        return Err("empty header name".into());
    }
    let n = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| format!("bad header name `{name}`: {e}"))?;
    let v = HeaderValue::from_str(value).map_err(|e| format!("bad header value: {e}"))?;
    Ok((n, v))
}
