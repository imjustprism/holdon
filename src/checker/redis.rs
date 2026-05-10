use url::Url;

use super::hint::hints;
use super::{AttemptCtx, run_stage};
use crate::diagnostic::{Stage, StageKind};

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    let conn_str = url.as_str();
    let pw = url.password().unwrap_or("");
    vec![
        run_stage(
            StageKind::Redis,
            ctx.attempt_timeout,
            hints::REDIS_NOT_READY,
            ping(conn_str),
            &[conn_str, pw],
        )
        .await,
    ]
}

async fn ping(conn_str: &str) -> redis::RedisResult<()> {
    let client = redis::Client::open(conn_str)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let _: () = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}
