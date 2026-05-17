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
    use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use tokio::sync::mpsc;
    use tokio::time::timeout;

    let parent = path.parent().filter(|p| !p.as_os_str().is_empty())?;
    // Use the async metadata API: std::path::Path::is_dir calls the
    // sync fs::metadata syscall, which would stall the Tokio worker on
    // a slow or network-mounted filesystem.
    let parent_is_dir = tokio::fs::metadata(parent)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false);
    if !parent_is_dir {
        // No usable directory to watch (e.g. /missing/file). Fall back
        // to the polling path.
        return None;
    }
    let target_file_name = path.file_name()?.to_owned();
    // Bounded channel so a busy parent directory cannot accumulate
    // unbounded notify events while we wait for the matching one.
    // try_send drops the oldest excess events, which is the right
    // behaviour for a readiness probe: we only need the next relevant
    // event, not the entire backlog.
    let (tx, mut rx) = mpsc::channel::<notify::Result<notify::Event>>(64);
    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(move |res| {
        let _ = tx.try_send(res);
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

    let want_create = matches!(mode, FileMode::Present);
    let deadline = ctx.attempt_timeout;
    let result = timeout(deadline, async {
        while let Some(event) = rx.recv().await {
            let Ok(event) = event else { continue };
            let relevant = event.paths.iter().any(|p| {
                p.file_name()
                    .is_some_and(|n| n == target_file_name.as_os_str())
            });
            if !relevant {
                continue;
            }
            // Atomic-create via `mv tmpfile target` arrives as
            // EventKind::Modify(ModifyKind::Name(RenameMode::To)) on
            // Linux (notify v8 folds renames under Modify, not a
            // top-level Rename variant). The Modify arm therefore
            // already covers IN_MOVED_TO and the rename-away
            // direction. The re-stat below decides authoritatively.
            let matches = match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => want_create,
                EventKind::Remove(_) => !want_create,
                _ => false,
            };
            if matches {
                if let Some(stage) = stat_once(path, mode, start).await {
                    return Some(stage);
                }
            }
        }
        None
    })
    .await;
    drop(watcher);
    result.ok().flatten().map(|s| vec![s])
}
