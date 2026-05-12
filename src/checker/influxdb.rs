use std::time::Instant;

use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::format_error_chain;

const PING_PATH: &str = "/ping";
const VERSION_HEADER: &str = "x-influxdb-version";
const BUILD_HEADER: &str = "x-influxdb-build";
const EXPECT_VERSION_KEY: &str = "expect-version";
const TOKEN_KEY: &str = "token";
const TOKEN_AUTH_PREFIX: &str = "Token ";

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let (request_url, want_version, token) = match prepare(url) {
        Ok(v) => v,
        Err(msg) => {
            return vec![err_stage(
                StageKind::Influxdb,
                start.elapsed(),
                msg,
                Some(hints::INFLUXDB_PARSE),
            )];
        }
    };
    let mut req = crate::checker::http::raw_client()
        .get(request_url)
        .timeout(ctx.attempt_timeout);
    if let Some(t) = &token {
        req = req.header("authorization", format!("{TOKEN_AUTH_PREFIX}{t}"));
    }
    let token_ref = token.as_deref().unwrap_or("");
    let stage = match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let version = resp
                .headers()
                .get(VERSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let build = resp
                .headers()
                .get(BUILD_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if !(status == 200 || status == 204) {
                let hint = if status == 401 {
                    hints::INFLUXDB_AUTH
                } else {
                    hints::INFLUXDB_NOT_READY
                };
                err_stage(
                    StageKind::Influxdb,
                    start.elapsed(),
                    format!("/ping returned status {status}"),
                    Some(hint),
                )
            } else if let Some(want) = want_version {
                check_version(version.as_deref(), build.as_deref(), want, start.elapsed())
            } else {
                ok_stage(StageKind::Influxdb, start.elapsed())
            }
        }
        Err(e) if e.is_timeout() => err_stage(
            StageKind::Influxdb,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::INFLUXDB_NOT_READY),
        ),
        Err(e) => {
            let mut msg = format_error_chain(&e);
            if !token_ref.is_empty() {
                msg = crate::util::redact_in(&msg, token_ref);
            }
            err_stage(
                StageKind::Influxdb,
                start.elapsed(),
                msg,
                Some(hints::INFLUXDB_NOT_READY),
            )
        }
    };
    vec![stage]
}

fn check_version(
    version: Option<&str>,
    build: Option<&str>,
    want: u8,
    elapsed: std::time::Duration,
) -> Stage {
    if let Some(v) = version {
        return if version_matches(v, want) {
            ok_stage(StageKind::Influxdb, elapsed)
        } else {
            err_stage(
                StageKind::Influxdb,
                elapsed,
                format!("server reports influxdb {v}, expected major {want}"),
                Some(hints::INFLUXDB_VERSION),
            )
        };
    }
    if let Some(b) = build {
        return if build_matches(b, want) {
            ok_stage(StageKind::Influxdb, elapsed)
        } else {
            err_stage(
                StageKind::Influxdb,
                elapsed,
                format!("server reports influxdb build `{b}`, expected major {want}"),
                Some(hints::INFLUXDB_VERSION),
            )
        };
    }
    err_stage(
        StageKind::Influxdb,
        elapsed,
        "server did not advertise X-Influxdb-Version or X-Influxdb-Build header",
        Some(hints::INFLUXDB_VERSION),
    )
}

fn prepare(url: &Url) -> Result<(Url, Option<u8>, Option<String>), String> {
    let mut want_version: Option<u8> = None;
    let mut token: Option<String> = None;
    for (k, v) in url.query_pairs() {
        if k.eq_ignore_ascii_case(EXPECT_VERSION_KEY) {
            match v.as_ref() {
                "1" => want_version = Some(1),
                "2" => want_version = Some(2),
                "3" => want_version = Some(3),
                other => {
                    return Err(format!(
                        "unknown influxdb:// expect-version `{other}` (only 1, 2, or 3)"
                    ));
                }
            }
        } else if k.eq_ignore_ascii_case(TOKEN_KEY) {
            if v.is_empty() {
                return Err("influxdb:// token cannot be empty".to_owned());
            }
            token = Some(v.into_owned());
        } else {
            return Err(format!(
                "unknown influxdb:// query key `{k}` (only `expect-version` and `token` supported)"
            ));
        }
    }
    let raw = url.as_str();
    let rewritten = if let Some(rest) = raw.strip_prefix("influxdb://") {
        format!("http://{rest}")
    } else if let Some(rest) = raw.strip_prefix("influxdbs://") {
        format!("https://{rest}")
    } else {
        return Err(format!(
            "influxdb probe: unexpected scheme `{}`",
            url.scheme()
        ));
    };
    let mut target =
        Url::parse(&rewritten).map_err(|e| format!("failed to rewrite influxdb:// URL: {e}"))?;
    target.set_query(None);
    let prefix = target.path().trim_end_matches('/');
    target.set_path(&format!("{prefix}{PING_PATH}"));
    Ok((target, want_version, token))
}

fn version_matches(reported: &str, want_major: u8) -> bool {
    reported
        .strip_prefix('v')
        .unwrap_or(reported)
        .split('.')
        .next()
        .and_then(|s| s.parse::<u8>().ok())
        .is_some_and(|m| m == want_major)
}

fn build_matches(reported: &str, want_major: u8) -> bool {
    let lower = reported.to_ascii_lowercase();
    match want_major {
        3 => lower.contains("core") || lower.contains("enterprise") || lower.starts_with("v3"),
        2 => lower.contains("oss") && !lower.contains("v1") && !lower.contains("core"),
        1 => lower.contains("v1"),
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_major_only() {
        assert!(version_matches("2.7.3", 2));
        assert!(version_matches("1.8.10", 1));
        assert!(version_matches("3.0.0", 3));
        assert!(!version_matches("2.7.3", 1));
        assert!(!version_matches("garbage", 2));
    }

    #[test]
    fn version_matches_strips_v_prefix() {
        assert!(version_matches("v2.7.3", 2));
        assert!(version_matches("v1.8", 1));
        assert!(version_matches("v3.1.0", 3));
    }

    #[test]
    fn build_matches_v3() {
        assert!(build_matches("Core", 3));
        assert!(build_matches("core", 3));
        assert!(build_matches("Enterprise", 3));
        assert!(!build_matches("Core", 2));
    }

    #[test]
    fn prepare_rewrites_to_http_and_strips_query() {
        let u: Url = "influxdb://host:8086?expect-version=2".parse().unwrap();
        let (out, want, token) = prepare(&u).unwrap();
        assert_eq!(out.scheme(), "http");
        assert_eq!(out.path(), PING_PATH);
        assert!(out.query().is_none());
        assert_eq!(want, Some(2));
        assert!(token.is_none());
    }

    #[test]
    fn prepare_rewrites_influxdbs_to_https() {
        let u: Url = "influxdbs://host:8086".parse().unwrap();
        let (out, _, _) = prepare(&u).unwrap();
        assert_eq!(out.scheme(), "https");
    }

    #[test]
    fn prepare_rejects_unknown_query_key() {
        let u: Url = "influxdb://host:8086?bucket=metrics".parse().unwrap();
        assert!(prepare(&u).is_err());
    }

    #[test]
    fn prepare_rejects_bad_version_value() {
        let u: Url = "influxdb://host:8086?expect-version=4".parse().unwrap();
        assert!(prepare(&u).is_err());
    }

    #[test]
    fn prepare_accepts_v3() {
        let u: Url = "influxdb://host:8086?expect-version=3".parse().unwrap();
        let (_, want, _) = prepare(&u).unwrap();
        assert_eq!(want, Some(3));
    }

    #[test]
    fn prepare_extracts_token() {
        let u: Url = "influxdb://host:8086?token=abc123".parse().unwrap();
        let (_, _, token) = prepare(&u).unwrap();
        assert_eq!(token.as_deref(), Some("abc123"));
    }

    #[test]
    fn prepare_rejects_empty_token() {
        let u: Url = "influxdb://host:8086?token=".parse().unwrap();
        assert!(prepare(&u).is_err());
    }

    #[test]
    fn prepare_preserves_path_prefix() {
        let u: Url = "influxdb://host:8086/proxy".parse().unwrap();
        let (out, _, _) = prepare(&u).unwrap();
        assert_eq!(out.path(), "/proxy/ping");
    }

    #[test]
    fn prepare_trims_trailing_slash_in_prefix() {
        let u: Url = "influxdb://host:8086/proxy/".parse().unwrap();
        let (out, _, _) = prepare(&u).unwrap();
        assert_eq!(out.path(), "/proxy/ping");
    }
}
