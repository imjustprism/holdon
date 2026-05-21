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

impl Runner {
    #[must_use]
    pub const fn new(cfg: RunnerConfig) -> Self {
        Self { cfg }
    }

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

        if matches!(self.cfg.schedule, Schedule::Sequential) {
            let mut results = Vec::with_capacity(targets.len());
            for (idx, target) in targets.into_iter().enumerate() {
                let dir = pick_direction(idx);
                let ov = pick_overrides(idx);
                let r = run_single(
                    idx,
                    target,
                    self.cfg.clone(),
                    dir,
                    ov,
                    deadline,
                    sink.as_ref(),
                )
                .await;
                results.push(r);
            }
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
            let s = sink.clone();
            set.spawn(
                async move { run_single(idx, target, cfg, dir, ov, deadline, s.as_ref()).await },
            );
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

#[tracing::instrument(skip_all, fields(idx, target = %target))]
async fn run_single(
    idx: usize,
    target: Target,
    cfg: RunnerConfig,
    direction: Direction,
    overrides: TargetOverrides,
    deadline: Instant,
    sink: Option<&EventSink>,
) -> TargetReport {
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

    TargetReport {
        idx,
        target,
        attempts,
        final_outcome,
        satisfied,
    }
}
