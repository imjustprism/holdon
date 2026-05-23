#![allow(clippy::unused_self)]

use std::io::{self, Write};
use std::time::SystemTime;

use serde_json::{Value, json};

use holdon::Target;
use holdon::diagnostic::{Stage, StageResult};
use holdon::runner::{Event, Report, TargetReport};
use holdon::util::duration_ms;

pub(crate) const VERSION: u8 = 1;

pub(crate) fn start_value(targets: &[Target], names: &[Option<String>]) -> Value {
    let entries: Vec<Value> = targets
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let name = names.get(i).and_then(Option::as_ref)?;
            Some(json!({"target": t.to_string(), "name": name}))
        })
        .collect();
    json!({
        "v": VERSION,
        "ts_unix_ms": now_unix_ms(),
        "event": "start",
        "targets": targets.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "checks": entries,
    })
}

pub(crate) fn event_value(ev: &Event) -> Option<Value> {
    if let Event::Attempt {
        target,
        attempt,
        latency,
        ready,
        ..
    } = ev
    {
        Some(json!({
            "v": VERSION,
            "ts_unix_ms": now_unix_ms(),
            "event": "attempt",
            "target": target.to_string(),
            "attempt": attempt,
            "latency_ms": duration_ms(*latency),
            "ready": ready,
        }))
    } else {
        None
    }
}

pub(crate) fn summary_values(report: &Report) -> Vec<Value> {
    let mut out: Vec<Value> = report.results.iter().map(target_event).collect();
    let ready_targets: Vec<String> = report
        .results
        .iter()
        .filter(|r| r.satisfied)
        .map(|r| r.target.to_string())
        .collect();
    let failed_targets: Vec<String> = report
        .results
        .iter()
        .filter(|r| !r.satisfied)
        .map(|r| r.target.to_string())
        .collect();
    out.push(json!({
        "v": VERSION,
        "ts_unix_ms": now_unix_ms(),
        "event": "end",
        "ready": report.all_ready(),
        "elapsed_ms": duration_ms(report.elapsed),
        "total": report.results.len(),
        "ready_targets": ready_targets,
        "failed_targets": failed_targets,
    }));
    out
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[derive(Debug)]
pub(crate) struct Json;

impl Json {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn banner(&self, targets: &[Target], names: &[Option<String>]) {
        emit(&start_value(targets, names));
    }

    pub(crate) fn event(&self, ev: &Event) {
        if let Some(v) = event_value(ev) {
            emit(&v);
        }
    }

    pub(crate) fn summary(&self, report: &Report) {
        for v in summary_values(report) {
            emit(&v);
        }
    }
}

pub(crate) fn emit_value(v: &Value) {
    emit(v);
}

fn target_event(r: &TargetReport) -> Value {
    json!({
        "v": VERSION,
        "ts_unix_ms": now_unix_ms(),
        "event": "target",
        "target": r.target.to_string(),
        "satisfied": r.satisfied,
        "ready": r.final_outcome.is_ready(),
        "attempts": r.attempts,
        "elapsed_ms": duration_ms(r.final_outcome.total),
        "stages": r.final_outcome.stages.iter().map(stage_value).collect::<Vec<_>>(),
    })
}

fn stage_value(s: &Stage) -> Value {
    let (status, message, hint) = if let StageResult::Err { message, hint } = &s.result {
        (
            "err",
            Some(message.as_ref()),
            hint.as_ref().map(AsRef::as_ref),
        )
    } else {
        ("ok", None, None)
    };
    json!({
        "kind": s.kind.as_str(),
        "status": status,
        "took_ms": duration_ms(s.took),
        "message": message,
        "hint": hint,
    })
}

fn emit(v: &Value) {
    let mut stdout = io::stdout().lock();
    if let Ok(s) = serde_json::to_string(v) {
        let _ = writeln!(stdout, "{s}");
    }
}
