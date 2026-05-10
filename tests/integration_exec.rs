#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod common;
use common::{quick_cfg, run};

use holdon::Target;
use holdon::diagnostic::{StageKind, StageResult};

/// On Windows we use `cmd /c exit <code>` for portable exit-status control.
/// On Unix we use `/bin/sh -c 'exit <code>'`.
#[cfg(windows)]
fn exit_target(code: i32) -> Target {
    format!("exec://cmd?arg=/c&arg=exit%20{code}")
        .parse()
        .unwrap()
}
#[cfg(not(windows))]
fn exit_target(code: i32) -> Target {
    format!("exec:///bin/sh?arg=-c&arg=exit%20{code}")
        .parse()
        .unwrap()
}

#[cfg(windows)]
fn stderr_target(line: &str) -> Target {
    // `cmd /c echo X 1>&2 & exit 1`
    let q = percent_encode_arg(&format!("echo {line} 1>&2 & exit 1"));
    format!("exec://cmd?arg=/c&arg={q}").parse().unwrap()
}
#[cfg(not(windows))]
fn stderr_target(line: &str) -> Target {
    let q = percent_encode_arg(&format!("printf '%s\\n' '{line}' 1>&2; exit 1"));
    format!("exec:///bin/sh?arg=-c&arg={q}").parse().unwrap()
}

fn percent_encode_arg(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[tokio::test]
async fn exec_exit_zero_is_ready() {
    let report = run(quick_cfg(3000), vec![exit_target(0)]).await;
    assert!(report.all_ready(), "expected ready on exit 0");
    let r = &report.results[0];
    let last = r.final_outcome.stages.last().unwrap();
    assert_eq!(last.kind, StageKind::Exec);
    assert!(matches!(last.result, StageResult::Ok));
}

#[tokio::test]
async fn exec_exit_nonzero_fails_and_carries_stderr() {
    let report = run(quick_cfg(1500), vec![stderr_target("DB-NOT-LISTENING-YET")]).await;
    assert!(!report.all_ready());
    let r = &report.results[0];
    let last = r.final_outcome.stages.last().unwrap();
    assert_eq!(last.kind, StageKind::Exec);
    let StageResult::Err { message, hint } = &last.result else {
        panic!("expected Err");
    };
    assert!(
        message.contains("DB-NOT-LISTENING-YET"),
        "stderr snippet missing from message: {message}"
    );
    let h = hint.as_ref().expect("hint set");
    assert!(
        h.contains("DB-NOT-LISTENING-YET") || h.contains("not-ready"),
        "hint: {h}"
    );
}

#[tokio::test]
async fn exec_program_not_found_has_helpful_hint() {
    let t: Target = "exec://holdon-definitely-not-a-real-binary-zzz"
        .parse()
        .unwrap();
    let report = run(quick_cfg(1500), vec![t]).await;
    assert!(!report.all_ready());
    let last = report.results[0].final_outcome.stages.last().unwrap();
    let StageResult::Err { hint, .. } = &last.result else {
        panic!("expected Err");
    };
    let h = hint.as_ref().expect("hint set");
    assert!(h.contains("not found"), "hint: {h}");
}

#[tokio::test]
async fn exec_stderr_ansi_escape_is_sanitized() {
    // Embed an ESC byte; the message must not contain a raw \x1b.
    #[cfg(windows)]
    let t: Target = "exec://cmd?arg=/c&arg=echo%20%1B%5B31mRED%1B%5B0m%201%3E%262%20%26%20exit%201"
        .parse()
        .unwrap();
    #[cfg(not(windows))]
    let t: Target =
        "exec:///bin/sh?arg=-c&arg=printf%20%27%5Cx1b%5B31mRED%5Cx1b%5B0m%5Cn%27%201%3E%262%3B%20exit%201"
            .parse()
            .unwrap();
    let report = run(quick_cfg(1500), vec![t]).await;
    let last = report.results[0].final_outcome.stages.last().unwrap();
    if let StageResult::Err { message, hint } = &last.result {
        assert!(
            !message.contains('\x1b'),
            "raw ESC leaked into message: {message:?}"
        );
        if let Some(h) = hint {
            assert!(!h.contains('\x1b'), "raw ESC leaked into hint: {h:?}");
        }
    }
}
