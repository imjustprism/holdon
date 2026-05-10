use std::path::Path;
use std::time::Instant;

use super::{Hintable, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::FileMode;
use crate::util::format_error_chain;

pub(super) async fn probe(path: &Path, mode: FileMode) -> Vec<Stage> {
    let start = Instant::now();
    let stage = match tokio::fs::symlink_metadata(path).await {
        Ok(_) => {
            if matches!(mode, FileMode::Present) {
                ok_stage(StageKind::File, start.elapsed())
            } else {
                err_stage(StageKind::File, start.elapsed(), "path still exists", None)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if matches!(mode, FileMode::Absent) {
                ok_stage(StageKind::File, start.elapsed())
            } else {
                err_stage(
                    StageKind::File,
                    start.elapsed(),
                    "path does not exist",
                    None,
                )
            }
        }
        Err(e) => err_stage(
            StageKind::File,
            start.elapsed(),
            format_error_chain(&e),
            e.hint(),
        ),
    };
    vec![stage]
}
