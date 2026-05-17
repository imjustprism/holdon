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
    // Initial state did not match. With the notify-fs feature, register an
    // OS-level watcher on the parent directory and react to the matching
    // create/remove event without polling. Without the feature, fall
    // through to a single err_stage so the runner retries with backoff.
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
    // Use the async metadata API: std::path::Path::is_dir calls the
    // sync fs::metadata syscall, which would stall the Tokio worker on
    // a slow or network-mounted filesystem.
    let parent_is_dir = tokio::fs::metadata(parent).await.is_ok_and(|m| m.is_dir());
    if !parent_is_dir {
        // No usable directory to watch (e.g. /missing/file). Fall back
        // to the polling path.
        return None;
    }
    // Notify coalesces multiple wakeups into one and never drops a
    // signal as long as the consumer eventually awaits notified()
    // again. We do not need to inspect individual events because the
    // post-event re-stat is authoritative for both create and remove,
    // and the cost of one extra stat per spurious wakeup is much
    // smaller than the cost of missing the one event we care about.
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

    // Re-stat after registering. Closes the race where the file changed
    // state between the first stat and the watcher being armed.
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
