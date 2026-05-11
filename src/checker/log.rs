use std::path::Path;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use super::hint::hints;
use super::{err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::LogMatcher;

/// Maximum bytes read from the log file per attempt. If the file is larger,
/// only the trailing `LOG_TAIL_BYTES` are scanned. Bounds memory and gives
/// "tail" semantics so a freshly appended `Listening on...` line surfaces
/// even when the historical log is huge.
const LOG_TAIL_BYTES: u64 = 1_024 * 1_024;

pub(super) async fn probe(path: &Path, matcher: &LogMatcher) -> Vec<Stage> {
    let start = Instant::now();
    let stage = match read_tail(path).await {
        Ok(content) => {
            if matcher_hits(matcher, &content) {
                ok_stage(StageKind::Log, start.elapsed())
            } else {
                err_stage(
                    StageKind::Log,
                    start.elapsed(),
                    matcher_miss_message(matcher),
                    Some(hints::LOG_NOT_YET),
                )
            }
        }
        Err(LogReadError::NotFound) => err_stage(
            StageKind::Log,
            start.elapsed(),
            "log file does not exist",
            Some(hints::LOG_PATH),
        ),
        Err(LogReadError::Io(msg)) => err_stage(
            StageKind::Log,
            start.elapsed(),
            format!("read failed: {msg}"),
            Some(hints::FILE_IO),
        ),
    };
    vec![stage]
}

fn matcher_hits(matcher: &LogMatcher, content: &str) -> bool {
    match matcher {
        LogMatcher::Substring(s) => content.contains(s.as_str()),
        LogMatcher::Regex(re) => re.is_match(content),
    }
}

fn matcher_miss_message(matcher: &LogMatcher) -> String {
    match matcher {
        LogMatcher::Substring(s) => format!("substring `{s}` not present in last 1 MiB"),
        LogMatcher::Regex(re) => {
            format!("regex `{}` did not match last 1 MiB", re.as_str())
        }
    }
}

enum LogReadError {
    NotFound,
    Io(String),
}

async fn read_tail(path: &Path) -> Result<String, LogReadError> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LogReadError::NotFound),
        Err(e) => return Err(LogReadError::Io(e.to_string())),
    };
    let len = file
        .metadata()
        .await
        .map_err(|e| LogReadError::Io(e.to_string()))?
        .len();
    if len > LOG_TAIL_BYTES {
        file.seek(SeekFrom::Start(len - LOG_TAIL_BYTES))
            .await
            .map_err(|e| LogReadError::Io(e.to_string()))?;
    }
    let cap = usize::try_from(LOG_TAIL_BYTES.min(len)).unwrap_or(usize::MAX);
    let mut buf = Vec::with_capacity(cap);
    // Bound the read at the computed cap so an actively-appended log cannot
    // make a single attempt over-read past the intended 1 MiB tail window.
    let mut limited = file.take(LOG_TAIL_BYTES);
    limited
        .read_to_end(&mut buf)
        .await
        .map_err(|e| LogReadError::Io(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
