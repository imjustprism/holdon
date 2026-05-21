use std::time::Instant;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

use super::{AttemptCtx, err_stage, hints, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::ProcessSelector;

pub(super) async fn probe(selector: &ProcessSelector, _ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    let selector = selector.clone();
    let stage = tokio::task::spawn_blocking(move || lookup(&selector, start))
        .await
        .unwrap_or_else(|_| {
            err_stage(
                StageKind::Process,
                start.elapsed(),
                "blocking task panicked while scanning processes",
                None,
            )
        });
    vec![stage]
}

fn lookup(selector: &ProcessSelector, start: Instant) -> Stage {
    let mut sys = System::new_with_specifics(RefreshKind::new());
    match selector {
        ProcessSelector::Pid(pid) => {
            let target = sysinfo::Pid::from_u32(*pid);
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[target]),
                false,
                ProcessRefreshKind::new(),
            );
            if sys.process(target).is_some() {
                ok_stage(StageKind::Process, start.elapsed())
            } else {
                err_stage(
                    StageKind::Process,
                    start.elapsed(),
                    format!("pid {pid} not running"),
                    Some(hints::PROCESS_NO_PID),
                )
            }
        }
        ProcessSelector::Name(name) => {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                false,
                ProcessRefreshKind::new().with_exe(UpdateKind::Always),
            );
            if any_process_matches(&sys, name) {
                ok_stage(StageKind::Process, start.elapsed())
            } else {
                err_stage(
                    StageKind::Process,
                    start.elapsed(),
                    format!("no process named `{name}` running"),
                    Some(hints::PROCESS_NO_NAME),
                )
            }
        }
    }
}

fn any_process_matches(sys: &System, wanted: &str) -> bool {
    sys.processes().values().any(|p| matches_name(p, wanted))
}

fn matches_name(proc: &sysinfo::Process, wanted: &str) -> bool {
    let raw = proc.name().to_string_lossy();
    if equal(&raw, wanted) {
        return true;
    }
    if let Some(stem) = strip_exe_suffix(&raw) {
        if equal(stem, wanted) {
            return true;
        }
    }
    if let Some(exe) = proc.exe() {
        if let Some(stem) = exe.file_stem().and_then(|s| s.to_str()) {
            if equal(stem, wanted) {
                return true;
            }
        }
    }
    false
}

fn strip_exe_suffix(s: &str) -> Option<&str> {
    let len = s.len();
    if len > 4 && s.as_bytes()[len - 4] == b'.' && s[len - 3..].eq_ignore_ascii_case("exe") {
        s.get(..len - 4)
    } else {
        None
    }
}

fn equal(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}
