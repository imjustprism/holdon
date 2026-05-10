use std::sync::OnceLock;
use std::time::Instant;

pub use reqwest::Method;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
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
    /// Custom CA certificates in PEM, appended to the bundled webpki roots.
    pub extra_ca_pem: Vec<Vec<u8>>,
    /// Minimum TLS protocol version. Defaults to TLS 1.2.
    pub min_tls: TlsMin,
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
            if let Ok(cert) = Certificate::from_pem(pem) {
                b = b.add_root_certificate(cert);
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
    let stage = match req.timeout(ctx.attempt_timeout).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if !expect.contains(status) {
                err_stage(
                    StageKind::Http,
                    start.elapsed(),
                    format!("status {status}"),
                    Some(hints::HTTP_RETRY),
                )
            } else if let Some(needle) = cfg.body_substring.as_deref() {
                match read_body_capped(resp).await {
                    Ok(body) if body.contains(needle) => ok_stage(StageKind::Http, start.elapsed()),
                    Ok(_) => err_stage(
                        StageKind::Http,
                        start.elapsed(),
                        "body did not contain expected substring",
                        Some(hints::HTTP_BODY_MISMATCH),
                    ),
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

async fn read_body_capped(mut resp: reqwest::Response) -> reqwest::Result<String> {
    let cap = usize::try_from(MAX_BODY_BYTES).unwrap_or(usize::MAX);
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
