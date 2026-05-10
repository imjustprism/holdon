use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;
use tokio::time::sleep;

use crate::checker::AttemptCtx;
use crate::diagnostic::CheckOutcome;
use crate::target::Target;

/// Whether the runner waits for targets to come up or to go down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Direction {
    /// Wait for the target to be ready (default).
    #[default]
    Wait,
    /// Wait for the target to be unreachable.
    Reverse,
}

/// Whether probes run side-by-side or one after the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Schedule {
    /// Probe every target concurrently (default).
    #[default]
    Parallel,
    /// Probe targets one after another, in input order, sharing the deadline.
    Sequential,
}

/// Knobs controlling how a [`Runner`] schedules and bounds probes.
///
/// Construct with [`RunnerConfig::default`] and chain the builder methods to
/// override individual fields. All durations are best-effort: in-flight probes
/// can overshoot the overall deadline by up to `attempt_timeout` because a
/// running probe is not interrupted mid-attempt.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunnerConfig {
    /// Wall-clock budget for the entire run, measured from the moment
    /// [`Runner::run`] is awaited (after `initial_delay`).
    pub overall_timeout: Duration,
    /// First retry interval after a failed probe. Each subsequent failure
    /// doubles the interval, capped at `max_interval`.
    pub initial_interval: Duration,
    /// Upper bound on the exponential backoff between retries.
    pub max_interval: Duration,
    /// Delay before the first probe fires.
    pub initial_delay: Duration,
    /// Per-attempt timeout for one full probe (DNS, TCP, handshake, query).
    pub attempt_timeout: Duration,
    /// Whether the readiness condition is normal or inverted.
    pub direction: Direction,
    /// When `true`, only one attempt is made per target. No retry loop.
    pub once: bool,
    /// Whether probes run in parallel or sequentially.
    pub schedule: Schedule,
    /// Number of consecutive successful attempts required before a target is
    /// considered satisfied. Default is 1. Higher values protect against
    /// flapping services that briefly return ready then fail.
    pub success_threshold: u32,
    /// When `true`, randomized jitter is applied to retry intervals to avoid
    /// thundering-herd lockstep across parallel runs. Default is true.
    pub jitter: bool,
}

impl RunnerConfig {
    /// Default total wall-clock budget for a [`Runner::run`] call.
    pub const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(30);
    /// Default first retry interval after a failed probe.
    pub const DEFAULT_INITIAL_INTERVAL: Duration = Duration::from_millis(100);
    /// Default upper bound on exponential backoff.
    pub const DEFAULT_MAX_INTERVAL: Duration = Duration::from_secs(2);
    /// Default delay before the first probe (none).
    pub const DEFAULT_INITIAL_DELAY: Duration = Duration::ZERO;
    /// Default per-attempt timeout.
    pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
    /// Default consecutive-success threshold.
    pub const DEFAULT_SUCCESS_THRESHOLD: u32 = 1;
    /// Floor applied to interval and attempt-timeout to avoid 0-duration spin.
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
        }
    }
}

impl RunnerConfig {
    /// Sets [`RunnerConfig::overall_timeout`].
    #[must_use]
    pub const fn timeout(mut self, d: Duration) -> Self {
        self.overall_timeout = d;
        self
    }
    /// Sets [`RunnerConfig::initial_interval`].
    #[must_use]
    pub const fn interval(mut self, d: Duration) -> Self {
        self.initial_interval = d;
        self
    }
    /// Sets [`RunnerConfig::max_interval`].
    #[must_use]
    pub const fn max_interval(mut self, d: Duration) -> Self {
        self.max_interval = d;
        self
    }
    /// Sets [`RunnerConfig::initial_delay`].
    #[must_use]
    pub const fn initial_delay(mut self, d: Duration) -> Self {
        self.initial_delay = d;
        self
    }
    /// Sets [`RunnerConfig::attempt_timeout`].
    #[must_use]
    pub const fn attempt_timeout(mut self, d: Duration) -> Self {
        self.attempt_timeout = d;
        self
    }
    /// Selects [`Direction::Reverse`] when `v` is true, otherwise
    /// [`Direction::Wait`].
    #[must_use]
    pub const fn reverse(mut self, v: bool) -> Self {
        self.direction = if v {
            Direction::Reverse
        } else {
            Direction::Wait
        };
        self
    }
    /// Sets [`RunnerConfig::once`].
    #[must_use]
    pub const fn once(mut self, v: bool) -> Self {
        self.once = v;
        self
    }
    /// Selects [`Schedule::Sequential`] when `v` is true, otherwise
    /// [`Schedule::Parallel`].
    #[must_use]
    pub const fn sequential(mut self, v: bool) -> Self {
        self.schedule = if v {
            Schedule::Sequential
        } else {
            Schedule::Parallel
        };
        self
    }
    /// Sets [`RunnerConfig::success_threshold`]. Clamps zero to one.
    #[must_use]
    pub const fn success_threshold(mut self, n: u32) -> Self {
        self.success_threshold = if n == 0 { 1 } else { n };
        self
    }
    /// Sets [`RunnerConfig::jitter`].
    #[must_use]
    pub const fn jitter(mut self, v: bool) -> Self {
        self.jitter = v;
        self
    }
}

/// Drives a set of [`Target`] probes to completion under a single deadline.
///
/// Construct with [`Runner::new`], then await [`Runner::run`]. The `Runner`
/// is consumed by `run` so a single instance cannot be reused.
#[derive(Debug)]
#[non_exhaustive]
pub struct Runner {
    cfg: RunnerConfig,
}

/// Per-target slice of a [`Report`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TargetReport {
    /// Position of this target in the original input vector. Reports are
    /// sorted by `idx` so output order matches input order even when probes
    /// finish out of order.
    pub idx: usize,
    /// The target as parsed. URL passwords are redacted by [`Target`]'s
    /// `Display` and `Debug` impls.
    pub target: Target,
    /// Number of probe attempts made before the loop terminated.
    pub attempts: u32,
    /// Final probe outcome.
    pub final_outcome: CheckOutcome,
    /// Whether the target met its readiness condition. Affected by
    /// [`RunnerConfig::reverse`].
    pub satisfied: bool,
}

/// Aggregate outcome of a [`Runner::run`] call.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Report {
    /// Per-target results, sorted by [`TargetReport::idx`].
    pub results: Vec<TargetReport>,
    /// Total wall-clock time of the run.
    pub elapsed: Duration,
}

impl Report {
    /// Returns `true` when there is at least one target and every target
    /// satisfied its readiness condition.
    ///
    /// An empty `Report` reports `false`. Vacuous-truth on an empty set is a
    /// footgun in test harnesses where forgetting to populate targets should
    /// not silently pass.
    #[must_use]
    pub fn all_ready(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(|r| r.satisfied)
    }

    /// Returns `Ok(())` when [`Report::all_ready`] is true, otherwise an
    /// error carrying the failed and total target counts.
    ///
    /// # Errors
    /// Returns [`crate::Error::NotReady`] if the report contains zero targets
    /// or if any target failed to satisfy its readiness condition.
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

/// Event emitted by [`Runner::run`] over the optional [`EventSink`].
///
/// Workers emit events from spawned tasks, so consumers must drain the
/// receiver concurrently with `run` to avoid back-pressure.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// Fires after every probe attempt with the latency and immediate result.
    Attempt {
        /// Position of the target in the original input.
        idx: usize,
        /// The target probed.
        target: Target,
        /// 1-based attempt counter.
        attempt: u32,
        /// Wall time the attempt took.
        latency: Duration,
        /// Whether this attempt's outcome was ready (pre-`reverse` semantics).
        ready: bool,
    },
    /// Fires once per target when the retry loop terminates.
    Finished {
        /// Position of the target in the original input.
        idx: usize,
        /// The target probed.
        target: Target,
        /// Total number of attempts made.
        attempts: u32,
        /// Final probe outcome.
        outcome: CheckOutcome,
        /// Whether the readiness condition was satisfied (after applying
        /// [`RunnerConfig::reverse`]).
        satisfied: bool,
    },
}

/// Channel sender used to receive [`Event`]s during a run.
///
/// Currently an unbounded sender. Consumers that may stall (a slow terminal,
/// a piped JSON consumer) should drain promptly to avoid memory growth
/// proportional to attempt rate × target count.
pub type EventSink = UnboundedSender<Event>;

impl Runner {
    /// Builds a [`Runner`] from a [`RunnerConfig`].
    #[must_use]
    pub const fn new(cfg: RunnerConfig) -> Self {
        Self { cfg }
    }

    /// Probes every target until each is satisfied or the overall deadline
    /// expires, then returns a [`Report`].
    ///
    /// In parallel mode (default), all targets are probed concurrently in
    /// independent tasks and share the overall deadline. In sequential mode,
    /// targets are probed one after another in input order, each consuming
    /// whatever budget remains.
    ///
    /// Pass `Some(sink)` to receive [`Event`]s as probes happen. The future is
    /// cancel-safe: dropping it aborts all in-flight probes and releases their
    /// sockets.
    #[tracing::instrument(skip_all, fields(targets = targets.len(), schedule = ?self.cfg.schedule))]
    pub async fn run(self, targets: Vec<Target>, sink: Option<EventSink>) -> Report {
        let started = Instant::now();
        let deadline = started + self.cfg.overall_timeout;
        tracing::debug!(timeout_ms = ?self.cfg.overall_timeout.as_millis(), "runner start");

        if !self.cfg.initial_delay.is_zero() {
            sleep(self.cfg.initial_delay).await;
        }

        if matches!(self.cfg.schedule, Schedule::Sequential) {
            let mut results = Vec::with_capacity(targets.len());
            for (idx, target) in targets.into_iter().enumerate() {
                let r = run_single(idx, target, self.cfg.clone(), deadline, sink.as_ref()).await;
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
            let s = sink.clone();
            set.spawn(async move { run_single(idx, target, cfg, deadline, s.as_ref()).await });
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
    deadline: Instant,
    sink: Option<&EventSink>,
) -> TargetReport {
    let attempt_ctx = AttemptCtx {
        attempt_timeout: cfg.attempt_timeout,
    };
    let mut interval = cfg.initial_interval.max(RunnerConfig::MIN_INTERVAL);
    let max_interval = cfg.max_interval.max(RunnerConfig::MIN_INTERVAL);
    let threshold = cfg.success_threshold.max(1);
    let mut attempts: u32 = 0;
    let mut consecutive_ok: u32 = 0;

    let (final_outcome, satisfied) = loop {
        attempts += 1;
        tracing::debug!(attempt = attempts, "probing");
        let outcome = target.probe(attempt_ctx).await;
        let one_ready = match cfg.direction {
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
