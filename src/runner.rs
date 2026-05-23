use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;
use tokio::time::sleep;

use crate::checker::AttemptCtx;
use crate::diagnostic::CheckOutcome;
use crate::target::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Direction {
    #[default]
    Wait,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Schedule {
    #[default]
    Parallel,
    Sequential,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunnerConfig {
    pub overall_timeout: Duration,
    pub initial_interval: Duration,
    pub max_interval: Duration,
    pub initial_delay: Duration,
    pub attempt_timeout: Duration,
    pub direction: Direction,
    pub once: bool,
    pub schedule: Schedule,
    pub success_threshold: u32,
    pub jitter: bool,
    pub directions: Option<Vec<Direction>>,
    pub overrides: Option<Vec<TargetOverrides>>,
    pub max_attempts: Option<u32>,
    pub prereqs: Option<Vec<Vec<usize>>>,
}

#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct TargetOverrides {
    pub interval: Option<Duration>,
    pub attempt_timeout: Option<Duration>,
    pub success_threshold: Option<u32>,
}

impl RunnerConfig {
    pub const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(30);

    pub const DEFAULT_INITIAL_INTERVAL: Duration = Duration::from_millis(100);

    pub const DEFAULT_MAX_INTERVAL: Duration = Duration::from_secs(2);

    pub const DEFAULT_INITIAL_DELAY: Duration = Duration::ZERO;

    pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

    pub const DEFAULT_SUCCESS_THRESHOLD: u32 = 1;

    pub const MIN_INTERVAL: Duration = Duration::from_millis(1);
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            overall_timeout: Self::DEFAULT_OVERALL_TIMEOUT,
            initial_interval: Self::DEFAULT_INITIAL_INTERVAL,
            max_interval: Self::DEFAULT_MAX_INTERVAL,
            initial_delay: Self::DEFAULT_INITIAL_DELAY,
            attempt_timeout: Self::DEFAULT_ATTEMPT_TIMEOUT,
            direction: Direction::Wait,
            once: false,
            schedule: Schedule::Parallel,
            success_threshold: Self::DEFAULT_SUCCESS_THRESHOLD,
            jitter: true,
            directions: None,
            overrides: None,
            max_attempts: None,
            prereqs: None,
        }
    }
}

impl RunnerConfig {
    #[must_use]
    pub const fn timeout(mut self, d: Duration) -> Self {
        self.overall_timeout = d;
        self
    }
    #[must_use]
    pub const fn interval(mut self, d: Duration) -> Self {
        self.initial_interval = d;
        self
    }
    #[must_use]
    pub const fn max_interval(mut self, d: Duration) -> Self {
        self.max_interval = d;
        self
    }
    #[must_use]
    pub const fn initial_delay(mut self, d: Duration) -> Self {
        self.initial_delay = d;
        self
    }
    #[must_use]
    pub const fn attempt_timeout(mut self, d: Duration) -> Self {
        self.attempt_timeout = d;
        self
    }
    #[must_use]
    pub const fn reverse(mut self, v: bool) -> Self {
        self.direction = if v {
            Direction::Reverse
        } else {
            Direction::Wait
        };
        self
    }
    #[must_use]
    pub const fn once(mut self, v: bool) -> Self {
        self.once = v;
        self
    }
    #[must_use]
    pub const fn sequential(mut self, v: bool) -> Self {
        self.schedule = if v {
            Schedule::Sequential
        } else {
            Schedule::Parallel
        };
        self
    }
    #[must_use]
    pub const fn success_threshold(mut self, n: u32) -> Self {
        self.success_threshold = if n == 0 { 1 } else { n };
        self
    }
    #[must_use]
    pub const fn jitter(mut self, v: bool) -> Self {
        self.jitter = v;
        self
    }

    #[must_use]
    pub fn directions(mut self, v: Option<Vec<Direction>>) -> Self {
        self.directions = v;
        self
    }

    #[must_use]
    pub fn overrides(mut self, v: Option<Vec<TargetOverrides>>) -> Self {
        self.overrides = v;
        self
    }

    #[must_use]
    pub const fn max_attempts(mut self, v: Option<u32>) -> Self {
        self.max_attempts = match v {
            Some(0) => Some(1),
            other => other,
        };
        self
    }

    #[must_use]
    pub fn prereqs(mut self, v: Option<Vec<Vec<usize>>>) -> Self {
        self.prereqs = v;
        self
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct Runner {
    cfg: RunnerConfig,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TargetReport {
    pub idx: usize,
    pub target: Target,
    pub attempts: u32,
    pub final_outcome: CheckOutcome,
    pub satisfied: bool,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Report {
    pub results: Vec<TargetReport>,
    pub elapsed: Duration,
}

impl Report {
    #[must_use]
    pub fn all_ready(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(|r| r.satisfied)
    }

    pub fn assert_all_ready(&self) -> crate::Result<()> {
        if self.all_ready() {
            Ok(())
        } else {
            let failed = self.results.iter().filter(|r| !r.satisfied).count();
            Err(crate::Error::NotReady {
                failed,
                total: self.results.len(),
            })
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    Attempt {
        idx: usize,
        target: Target,
        attempt: u32,
        latency: Duration,
        ready: bool,
    },
    Finished {
        idx: usize,
        target: Target,
        attempts: u32,
        outcome: CheckOutcome,
        satisfied: bool,
    },
}

pub type EventSink = UnboundedSender<Event>;

#[derive(Debug)]
struct PrereqChannel {
    tx: tokio::sync::watch::Sender<Option<bool>>,
    rx: tokio::sync::watch::Receiver<Option<bool>>,
}

impl Runner {
    #[must_use]
    pub const fn new(cfg: RunnerConfig) -> Self {
        Self { cfg }
    }

    #[allow(clippy::too_many_lines)]
    #[tracing::instrument(skip_all, fields(targets = targets.len(), schedule = ?self.cfg.schedule))]
    pub async fn run(self, targets: Vec<Target>, sink: Option<EventSink>) -> Report {
        let started = Instant::now();
        let deadline = started + self.cfg.overall_timeout;
        tracing::debug!(timeout_ms = ?self.cfg.overall_timeout.as_millis(), "runner start");

        if !self.cfg.initial_delay.is_zero() {
            sleep(self.cfg.initial_delay).await;
        }

        let pick_direction = |idx: usize| -> Direction {
            self.cfg
                .directions
                .as_ref()
                .and_then(|v| v.get(idx).copied())
                .unwrap_or(self.cfg.direction)
        };
        let pick_overrides = |idx: usize| -> TargetOverrides {
            self.cfg
                .overrides
                .as_ref()
                .and_then(|v| v.get(idx).cloned())
                .unwrap_or_default()
        };
        let target_count_initial = targets.len();
        let prereq_signals: Vec<PrereqChannel> = (0..target_count_initial)
            .map(|_| {
                let (tx, rx) = tokio::sync::watch::channel(None);
                PrereqChannel { tx, rx }
            })
            .collect();
        let pick_prereq_rxs = |idx: usize| -> Vec<tokio::sync::watch::Receiver<Option<bool>>> {
            self.cfg
                .prereqs
                .as_ref()
                .and_then(|v| v.get(idx))
                .map(|deps| {
                    deps.iter()
                        .filter_map(|&i| prereq_signals.get(i).map(|c| c.rx.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };

        if matches!(self.cfg.schedule, Schedule::Sequential) {
            let order = topo_order(self.cfg.prereqs.as_ref(), targets.len());
            let mut owned: Vec<Option<Target>> = targets.into_iter().map(Some).collect();
            let mut results = Vec::with_capacity(owned.len());
            for idx in order {
                let Some(target) = owned[idx].take() else {
                    continue;
                };
                let dir = pick_direction(idx);
                let ov = pick_overrides(idx);
                let prereq_rxs = pick_prereq_rxs(idx);
                let signal = prereq_signals.get(idx).map(|c| c.tx.clone());
                let r = run_single(
                    idx,
                    target,
                    self.cfg.clone(),
                    dir,
                    ov,
                    prereq_rxs,
                    signal,
                    deadline,
                    sink.as_ref(),
                )
                .await;
                results.push(r);
            }
            results.sort_by_key(|r| r.idx);
            return Report {
                results,
                elapsed: started.elapsed(),
            };
        }

        let target_count = targets.len();
        let mut set: JoinSet<TargetReport> = JoinSet::new();
        for (idx, target) in targets.into_iter().enumerate() {
            let cfg = self.cfg.clone();
            let dir = pick_direction(idx);
            let ov = pick_overrides(idx);
            let prereq_rxs = pick_prereq_rxs(idx);
            let signal = prereq_signals.get(idx).map(|c| c.tx.clone());
            let s = sink.clone();
            set.spawn(async move {
                run_single(
                    idx,
                    target,
                    cfg,
                    dir,
                    ov,
                    prereq_rxs,
                    signal,
                    deadline,
                    s.as_ref(),
                )
                .await
            });
        }

        let mut results = Vec::with_capacity(target_count);
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(r) => results.push(r),
                Err(e) => {
                    tracing::error!(error = %e, "probe task failed");
                }
            }
        }
        results.sort_by_key(|r| r.idx);
        Report {
            results,
            elapsed: started.elapsed(),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
#[tracing::instrument(skip_all, fields(idx, target = %target))]
async fn run_single(
    idx: usize,
    target: Target,
    cfg: RunnerConfig,
    direction: Direction,
    overrides: TargetOverrides,
    prereq_rxs: Vec<tokio::sync::watch::Receiver<Option<bool>>>,
    done_signal: Option<tokio::sync::watch::Sender<Option<bool>>>,
    deadline: Instant,
    sink: Option<&EventSink>,
) -> TargetReport {
    if !prereq_rxs.is_empty() {
        for mut rx in prereq_rxs {
            loop {
                let current = *rx.borrow_and_update();
                match current {
                    Some(true) => break,
                    Some(false) => {
                        let report = prereq_failed_report(idx, target.clone());
                        if let Some(sig) = done_signal {
                            let _ = sig.send(Some(false));
                        }
                        return report;
                    }
                    None => {
                        let now = Instant::now();
                        if now >= deadline {
                            let report = prereq_deadline_report(idx, target.clone());
                            if let Some(sig) = done_signal {
                                let _ = sig.send(Some(false));
                            }
                            return report;
                        }
                        let changed = tokio::select! {
                            r = rx.changed() => r.is_ok(),
                            () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => false,
                        };
                        if !changed {
                            let report = prereq_deadline_report(idx, target.clone());
                            if let Some(sig) = done_signal {
                                let _ = sig.send(Some(false));
                            }
                            return report;
                        }
                    }
                }
            }
        }
    }
    let attempt_ctx = AttemptCtx {
        attempt_timeout: overrides
            .attempt_timeout
            .unwrap_or(cfg.attempt_timeout)
            .max(RunnerConfig::MIN_INTERVAL),
    };
    let initial_interval = overrides.interval.unwrap_or(cfg.initial_interval);
    let mut interval = initial_interval.max(RunnerConfig::MIN_INTERVAL);
    let max_interval = cfg.max_interval.max(RunnerConfig::MIN_INTERVAL);
    let threshold = overrides
        .success_threshold
        .unwrap_or(cfg.success_threshold)
        .max(1);
    let mut attempts: u32 = 0;
    let mut consecutive_ok: u32 = 0;

    let (final_outcome, satisfied) = loop {
        attempts += 1;
        tracing::debug!(attempt = attempts, "probing");
        let outcome = target.probe(attempt_ctx).await;
        let one_ready = match direction {
            Direction::Wait => outcome.is_ready(),
            Direction::Reverse => !outcome.is_ready(),
        };
        consecutive_ok = if one_ready { consecutive_ok + 1 } else { 0 };
        if let Some(s) = sink {
            let _ = s.send(Event::Attempt {
                idx,
                target: target.clone(),
                attempt: attempts,
                latency: outcome.total,
                ready: one_ready,
            });
        }

        let satisfied = consecutive_ok >= threshold;
        if satisfied || cfg.once {
            break (outcome, satisfied);
        }
        if cfg.max_attempts.is_some_and(|cap| attempts >= cap) {
            break (outcome, false);
        }
        let now = Instant::now();
        if now >= deadline {
            break (outcome, false);
        }
        let mut wait = interval
            .min(deadline.saturating_duration_since(now))
            .min(max_interval);
        if cfg.jitter && !wait.is_zero() {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let micros = wait.as_micros() as u64;
            let jittered = fastrand::u64(0..=micros);
            wait = Duration::from_micros(jittered);
        }
        sleep(wait).await;
        interval = interval.saturating_mul(2).min(max_interval);
    };

    if satisfied {
        tracing::debug!(attempts, elapsed_ms = ?final_outcome.total.as_millis(), "ready");
    } else {
        tracing::debug!(attempts, "deadline exceeded");
    }
    if let Some(s) = sink {
        let _ = s.send(Event::Finished {
            idx,
            target: target.clone(),
            attempts,
            outcome: final_outcome.clone(),
            satisfied,
        });
    }
    if let Some(sig) = done_signal {
        let _ = sig.send(Some(satisfied));
    }

    TargetReport {
        idx,
        target,
        attempts,
        final_outcome,
        satisfied,
    }
}

fn topo_order(prereqs: Option<&Vec<Vec<usize>>>, count: usize) -> Vec<usize> {
    let Some(graph) = prereqs else {
        return (0..count).collect();
    };
    let mut indeg: Vec<usize> = vec![0; count];
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (node, deps) in graph.iter().enumerate().take(count) {
        for &dep in deps {
            if dep < count {
                indeg[node] += 1;
                rev[dep].push(node);
            }
        }
    }
    let mut queue: std::collections::VecDeque<usize> =
        (0..count).filter(|&i| indeg[i] == 0).collect();
    let mut out: Vec<usize> = Vec::with_capacity(count);
    while let Some(n) = queue.pop_front() {
        out.push(n);
        for &m in &rev[n] {
            indeg[m] -= 1;
            if indeg[m] == 0 {
                queue.push_back(m);
            }
        }
    }
    for i in 0..count {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

fn prereq_failed_report(idx: usize, target: Target) -> TargetReport {
    let stage = crate::diagnostic::Stage {
        kind: crate::diagnostic::StageKind::Exec,
        took: Duration::ZERO,
        result: crate::diagnostic::StageResult::Err {
            message: "skipped because a prerequisite check did not become ready".into(),
            hint: Some("fix the upstream target listed in `after = [...]`".into()),
        },
    };
    TargetReport {
        idx,
        target,
        attempts: 0,
        final_outcome: CheckOutcome::failed(vec![stage], Duration::ZERO),
        satisfied: false,
    }
}

fn prereq_deadline_report(idx: usize, target: Target) -> TargetReport {
    let stage = crate::diagnostic::Stage {
        kind: crate::diagnostic::StageKind::Exec,
        took: Duration::ZERO,
        result: crate::diagnostic::StageResult::Err {
            message: "overall deadline expired while waiting for a prerequisite".into(),
            hint: Some("raise --timeout or speed up the upstream check".into()),
        },
    };
    TargetReport {
        idx,
        target,
        attempts: 0,
        final_outcome: CheckOutcome::failed(vec![stage], Duration::ZERO),
        satisfied: false,
    }
}
