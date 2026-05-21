use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, sanitize_for_terminal};

const STDERR_SNIPPET_MAX: usize = 200;
const STDERR_BUF_MAX: usize = 8 * 1024;

pub(super) async fn probe(program: &Path, args: &[String], ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let hint = if matches!(e.kind(), std::io::ErrorKind::NotFound) {
                Some(hints::EXEC_NOT_FOUND)
            } else if matches!(e.kind(), std::io::ErrorKind::PermissionDenied) {
                Some(hints::EXEC_PERMISSION)
            } else {
                None
            };
            return vec![err_stage(
                StageKind::Exec,
                start.elapsed(),
                format!("spawn `{}`: {}", program.display(), format_error_chain(&e)),
                hint,
            )];
        }
    };

    match timeout(ctx.attempt_timeout, wait_with_stderr(child)).await {
        Ok(Ok(WaitResult { status, stderr })) => {
            let took = start.elapsed();
            if status.success() {
                return vec![ok_stage(StageKind::Exec, took)];
            }
            let code_str = status
                .code()
                .map_or_else(|| "no exit code (signal)".to_owned(), |c| format!("{c}"));
            let snippet = stderr_snippet(&stderr);
            let first_line = first_nonempty_line(&stderr);
            let msg = if snippet.is_empty() {
                format!("command exited {code_str}")
            } else {
                format!("command exited {code_str}: {snippet}")
            };
            let hint_str: Option<Box<str>> = if first_line.is_empty() {
                Some(hints::EXEC_NONZERO.into())
            } else {
                Some(format!("stderr: {first_line}").into_boxed_str())
            };
            vec![Stage {
                kind: StageKind::Exec,
                took,
                result: crate::diagnostic::StageResult::Err {
                    message: msg.into(),
                    hint: hint_str,
                },
            }]
        }
        Ok(Err(e)) => vec![err_stage(
            StageKind::Exec,
            start.elapsed(),
            format!("wait: {}", format_error_chain(&e)),
            None,
        )],
        Err(_) => vec![err_stage(
            StageKind::Exec,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::EXEC_TIMED_OUT),
        )],
    }
}

struct WaitResult {
    status: std::process::ExitStatus,
    stderr: Vec<u8>,
}

async fn wait_with_stderr(mut child: tokio::process::Child) -> std::io::Result<WaitResult> {
    let stderr_handle = child.stderr.take();
    let stderr_fut = async move {
        let mut buf = Vec::new();
        if let Some(mut s) = stderr_handle {
            let mut chunk = [0u8; 1024];
            loop {
                if buf.len() >= STDERR_BUF_MAX {
                    break;
                }
                match s.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let take = n.min(STDERR_BUF_MAX - buf.len());
                        buf.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        buf
    };
    let (status, stderr) = tokio::join!(child.wait(), stderr_fut);
    Ok(WaitResult {
        status: status?,
        stderr,
    })
}

fn stderr_snippet(buf: &[u8]) -> String {
    if buf.is_empty() {
        return String::new();
    }
    let take = buf.len().min(STDERR_SNIPPET_MAX);
    let s = String::from_utf8_lossy(&buf[..take]);
    let mut out = sanitize_for_terminal(&s);
    if buf.len() > take {
        out.push('…');
    }
    out
}

fn first_nonempty_line(buf: &[u8]) -> String {
    let s = String::from_utf8_lossy(buf);
    for line in s.lines() {
        let t = line.trim();
        if !t.is_empty() {
            let truncated: String = t.chars().take(STDERR_SNIPPET_MAX).collect();
            return sanitize_for_terminal(&truncated);
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_nonempty_line_handles_multibyte_at_truncation_boundary() {
        let stderr: Vec<u8> = "é".repeat(300).into_bytes();
        let line = first_nonempty_line(&stderr);
        assert!(line.chars().count() <= STDERR_SNIPPET_MAX);
        assert!(line.starts_with("é"));
    }

    #[test]
    fn first_nonempty_line_strips_control_bytes() {
        let stderr = b"\x1b[31mERROR\x1b[0m boom";
        let line = first_nonempty_line(stderr);
        assert!(!line.contains('\x1b'));
        assert!(line.contains("ERROR"));
    }
}
