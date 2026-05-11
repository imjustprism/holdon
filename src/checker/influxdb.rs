use std::time::Instant;

use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::format_error_chain;

const PING_PATH: &str = "/ping";
const VERSION_HEADER: &str = "x-influxdb-version";
const EXPECT_VERSION_KEY: &str = "expect-version";

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let (request_url, want_version) = match prepare(url) {
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
    let stage = match crate::checker::http::raw_client()
        .get(request_url)
        .timeout(ctx.attempt_timeout)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let version = resp
                .headers()
                .get(VERSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if !(status == 200 || status == 204) {
                err_stage(
                    StageKind::Influxdb,
                    start.elapsed(),
                    format!("/ping returned status {status}"),
                    Some(hints::INFLUXDB_NOT_READY),
                )
            } else if let Some(want) = want_version {
                match version.as_deref() {
                    Some(v) if version_matches(v, want) => {
                        ok_stage(StageKind::Influxdb, start.elapsed())
                    }
                    Some(v) => err_stage(
                        StageKind::Influxdb,
                        start.elapsed(),
                        format!("server reports influxdb {v}, expected major {want}"),
                        Some(hints::INFLUXDB_VERSION),
                    ),
                    None => err_stage(
                        StageKind::Influxdb,
                        start.elapsed(),
                        "server did not advertise X-Influxdb-Version header",
                        Some(hints::INFLUXDB_VERSION),
                    ),
                }
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
        Err(e) => err_stage(
            StageKind::Influxdb,
            start.elapsed(),
            format_error_chain(&e),
            Some(hints::INFLUXDB_NOT_READY),
        ),
    };
    vec![stage]
}

fn prepare(url: &Url) -> Result<(Url, Option<u8>), String> {
    let mut want_version: Option<u8> = None;
    for (k, v) in url.query_pairs() {
        if k.eq_ignore_ascii_case(EXPECT_VERSION_KEY) {
            match v.as_ref() {
                "1" => want_version = Some(1),
                "2" => want_version = Some(2),
                other => {
                    return Err(format!(
                        "unknown influxdb:// expect-version `{other}` (only 1 or 2)"
                    ));
                }
            }
        } else {
            return Err(format!(
                "unknown influxdb:// query key `{k}` (only `expect-version` supported)"
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
    Ok((target, want_version))
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_major_only() {
        assert!(version_matches("2.7.3", 2));
        assert!(version_matches("1.8.10", 1));
        assert!(!version_matches("2.7.3", 1));
        assert!(!version_matches("garbage", 2));
    }

    #[test]
    fn version_matches_strips_v_prefix() {
        assert!(version_matches("v2.7.3", 2));
        assert!(version_matches("v1.8", 1));
    }

    #[test]
    fn prepare_preserves_path_prefix() {
        let u: Url = "influxdb://host:8086/proxy".parse().unwrap();
        let (out, _) = prepare(&u).unwrap();
        assert_eq!(out.path(), "/proxy/ping");
    }

    #[test]
    fn prepare_trims_trailing_slash_in_prefix() {
        let u: Url = "influxdb://host:8086/proxy/".parse().unwrap();
        let (out, _) = prepare(&u).unwrap();
        assert_eq!(out.path(), "/proxy/ping");
    }

    #[test]
    fn prepare_rewrites_to_http_and_strips_query() {
        let u: Url = "influxdb://host:8086?expect-version=2".parse().unwrap();
        let (out, want) = prepare(&u).unwrap();
        assert_eq!(out.scheme(), "http");
        assert_eq!(out.path(), PING_PATH);
        assert!(out.query().is_none());
        assert_eq!(want, Some(2));
    }

    #[test]
    fn prepare_rewrites_influxdbs_to_https() {
        let u: Url = "influxdbs://host:8086".parse().unwrap();
        let (out, _) = prepare(&u).unwrap();
        assert_eq!(out.scheme(), "https");
    }

    #[test]
    fn prepare_rejects_unknown_query_key() {
        let u: Url = "influxdb://host:8086?bucket=metrics".parse().unwrap();
        assert!(prepare(&u).is_err());
    }

    #[test]
    fn prepare_rejects_bad_version_value() {
        let u: Url = "influxdb://host:8086?expect-version=3".parse().unwrap();
        assert!(prepare(&u).is_err());
    }
}
