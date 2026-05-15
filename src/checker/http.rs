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

const MAX_BODY_BYTES: u64 = 1_024 * 1_024;
const FAILURE_BODY_SNIPPET_BYTES: usize = 240;
const SERVER_HINT_HEADERS: &[&str] = &["server", "x-powered-by", "via"];
const SERVER_HINT_VALUE_MAX: usize = 80;

/// Minimum TLS protocol version accepted by HTTPS probes.
///
/// `V12` is the default and matches the current IETF guidance, which
/// deprecates TLS 1.0 and 1.1. `V13` opts into a strict floor for callers
/// that want to refuse anything older than TLS 1.3.
///
/// The setting applies to every HTTPS target probed by the shared
/// reqwest client. There is no per-target override.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TlsMin {
    #[default]
    V12,
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
    pub headers: HeaderMap,
    pub method: Method,
    pub insecure: bool,
    pub follow_redirects: bool,
    pub body_substring: Option<String>,
    pub body_regex: Option<regex_lite::Regex>,
    pub body_json_match: Option<(String, String)>,
    pub extra_ca_pem: Vec<Vec<u8>>,
    pub min_tls: TlsMin,
    pub body: Option<Vec<u8>>,
    pub client_identity_pem: Option<Vec<u8>>,
    pub header_expectations: Vec<(HeaderName, regex_lite::Regex)>,
    pub http2_prior_knowledge: bool,
}

impl HttpConfig {
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
        if cfg.http2_prior_knowledge {
            b = b.http2_prior_knowledge();
        }
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

fn truncate_ellipsis(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn upstream_hint(headers: &HeaderMap) -> Option<String> {
    let parts: Vec<String> = SERVER_HINT_HEADERS
        .iter()
        .filter_map(|name| {
            let value = headers.get(*name)?.to_str().ok()?;
            let cleaned = crate::util::sanitize_for_terminal(value);
            let trimmed = cleaned.trim();
            (!trimmed.is_empty()).then(|| {
                format!(
                    "{name}: {}",
                    truncate_ellipsis(trimmed, SERVER_HINT_VALUE_MAX)
                )
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

async fn read_body_snippet(resp: reqwest::Response) -> String {
    let raw = read_body_to(resp, FAILURE_BODY_SNIPPET_BYTES * 4)
        .await
        .unwrap_or_default();
    if raw.is_empty() {
        return String::new();
    }
    let cleaned = crate::util::sanitize_for_terminal(&raw);
    let compact = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_ellipsis(&compact, FAILURE_BODY_SNIPPET_BYTES)
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
