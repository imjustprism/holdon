use std::sync::OnceLock;
use std::time::Instant;

use reqwest::Client;
pub use reqwest::Method;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::StatusRange;
use crate::util::{format_error_chain, redact_in};

/// Process-wide HTTP request configuration applied to every `http(s)://` probe.
///
/// Set once from the CLI layer via [`set_global`] before the first probe.
/// Library users that need per-target headers can leave this unset and use
/// the defaults: `GET`, no extra headers, TLS verification on.
#[derive(Debug, Default, Clone)]
pub struct HttpConfig {
    /// Extra request headers, applied to every HTTP probe.
    pub headers: HeaderMap,
    /// HTTP method. Defaults to `GET`.
    pub method: Method,
    /// When true, TLS certificate verification is disabled. Use only for
    /// self-signed development endpoints.
    pub insecure: bool,
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
    CONFIG.get_or_init(HttpConfig::default)
}

fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
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
        });
        let mut b = Client::builder()
            .user_agent(concat!("holdon/", env!("CARGO_PKG_VERSION")))
            .redirect(policy);
        if config().insecure {
            b = b.danger_accept_invalid_certs(true);
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
            if expect.contains(status) {
                ok_stage(StageKind::Http, start.elapsed())
            } else {
                err_stage(
                    StageKind::Http,
                    start.elapsed(),
                    format!("status {status}"),
                    Some(hints::HTTP_RETRY),
                )
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
