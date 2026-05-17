use std::time::Duration;

use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, err_stage, run_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::{LogMatcher, RedisKeyExpect};

pub(super) async fn probe(
    url: &Url,
    expect_key: Option<&RedisKeyExpect>,
    ctx: AttemptCtx,
) -> Vec<Stage> {
    let pw = url.password().unwrap_or("").to_owned();
    if !pw.is_empty() && !url.scheme().eq_ignore_ascii_case("rediss") {
        return vec![err_stage(
            StageKind::Redis,
            Duration::ZERO,
            "refusing to send AUTH over a non-TLS redis:// URL",
            Some(hints::CLEARTEXT_CREDS),
        )];
    }
    let driver_url = super::strip_query_keys(url, &["key", "match", "regex"]);
    let driver_str = driver_url.as_str().to_owned();
    let expect = expect_key.cloned();
    vec![
        run_stage(
            StageKind::Redis,
            ctx.attempt_timeout,
            hints::REDIS_NOT_READY,
            check(driver_str.clone(), expect),
            &[driver_str.as_str(), pw.as_str()],
        )
        .await,
    ]
}

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error(transparent)]
    Driver(#[from] redis::RedisError),
    #[error("expected key `{0}` not present")]
    KeyMissing(String),
    #[error("value for key `{0}` did not contain expected substring")]
    ValueSubstring(String),
    #[error("value for key `{key}` did not match regex `{regex}`")]
    ValueRegex { key: String, regex: String },
}

impl Hintable for ProbeError {
    fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Driver(e) => e.hint(),
            Self::KeyMissing(_) => Some(hints::REDIS_KEY_MISSING),
            Self::ValueSubstring(_) | Self::ValueRegex { .. } => Some(hints::REDIS_VALUE_MISMATCH),
        }
    }
}

async fn check(conn_str: String, expect: Option<RedisKeyExpect>) -> Result<(), ProbeError> {
    let client = redis::Client::open(conn_str)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let _: () = redis::cmd("PING").query_async(&mut conn).await?;
    let Some(expect) = expect else {
        return Ok(());
    };
    let value: Option<Vec<u8>> = redis::cmd("GET")
        .arg(&expect.key)
        .query_async(&mut conn)
        .await?;
    let Some(bytes) = value else {
        return Err(ProbeError::KeyMissing(expect.key));
    };
    let Some(matcher) = expect.matcher else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&bytes);
    match matcher {
        LogMatcher::Substring(needle) => {
            if text.contains(needle.as_str()) {
                Ok(())
            } else {
                Err(ProbeError::ValueSubstring(expect.key))
            }
        }
        LogMatcher::Regex(re) => {
            if re.is_match(&text) {
                Ok(())
            } else {
                Err(ProbeError::ValueRegex {
                    key: expect.key,
                    regex: re.as_str().to_owned(),
                })
            }
        }
    }
}
