use std::path::Path;
use std::time::Instant;

use super::{AttemptCtx, Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::FileMode;
use crate::util::format_error_chain;

pub(super) async fn probe(path: &Path, mode: FileMode, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    if let Some(stage) = stat_once(path, mode, start).await {
        return vec![stage];
    }
    #[cfg(feature = "notify-fs")]
    {
        if let Some(stage) = wait_for_event(path, mode, ctx, start).await {
            return stage;
        }
    }
    #[cfg(not(feature = "notify-fs"))]
    {
        let _ = ctx;
    }
    vec![not_matched(path, mode, start)]
}

async fn stat_once(path: &Path, mode: FileMode, start: Instant) -> Option<Stage> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => match mode {
            FileMode::Present => Some(ok_stage(StageKind::File, start.elapsed())),
            FileMode::Absent => None,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match mode {
            FileMode::Absent => Some(ok_stage(StageKind::File, start.elapsed())),
            FileMode::Present => None,
        },
        Err(e) => Some(err_stage(
            StageKind::File,
            start.elapsed(),
            format_error_chain(&e),
            e.hint(),
        )),
    }
}

fn not_matched(path: &Path, mode: FileMode, start: Instant) -> Stage {
    let msg = match mode {
        FileMode::Present => "path does not exist",
        FileMode::Absent => "path still exists",
    };
    let _ = path;
    err_stage(StageKind::File, start.elapsed(), msg, None)
}

#[cfg(feature = "notify-fs")]
async fn wait_for_event(
    path: &Path,
    mode: FileMode,
    ctx: AttemptCtx,
    start: Instant,
) -> Option<Vec<Stage>> {
    use std::sync::Arc;

    use notify::{RecommendedWatcher, RecursiveMode, Watcher};
    use tokio::sync::Notify;
    use tokio::time::timeout;

    let parent = path.parent().filter(|p| !p.as_os_str().is_empty())?;
    let parent_is_dir = tokio::fs::metadata(parent).await.is_ok_and(|m| m.is_dir());
    if !parent_is_dir {
        return None;
    }
    let bell = Arc::new(Notify::new());
    let kicker = bell.clone();
    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(move |_res| {
        kicker.notify_one();
    }) {
        Ok(w) => w,
        Err(_) => return None,
    };
    if watcher.watch(parent, RecursiveMode::NonRecursive).is_err() {
        return None;
    }

    if let Some(stage) = stat_once(path, mode, start).await {
        return Some(vec![stage]);
    }

    let deadline = ctx.attempt_timeout;
    let result = timeout(deadline, async {
        loop {
            bell.notified().await;
            if let Some(stage) = stat_once(path, mode, start).await {
                return Some(stage);
            }
        }
    })
    .await;
    drop(watcher);
    result.ok().flatten().map(|s| vec![s])
}
