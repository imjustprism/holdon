use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;
use tokio::time::sleep;

use crate::checker::AttemptCtx;
use crate::diagnostic::CheckOutcome;
use crate::target::Target;

/// Direction the runner moves towards readiness.
///
/// `Wait` is the default. The probe keeps retrying until the target reports
/// ready, the overall deadline expires, or the consecutive-success threshold
/// is reached.
///
/// `Reverse` flips the condition. The probe keeps retrying until the target
/// reports NOT ready. Useful for teardown scripts that need to confirm a
/// port has stopped listening or a service has finished draining before
/// they move on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Direction {
    #[default]
    Wait,
    Reverse,
}

/// Whether targets are probed concurrently or one after another.
///
/// `Parallel` is the default. Every target runs in its own task and shares
/// the overall deadline. Total wall-clock is bounded by the slowest target.
///
/// `Sequential` walks the input in order. Each target consumes whatever time
/// remains under the overall deadline. Useful when a later target depends on
/// the previous one already being up, or when you want predictable log
/// output without interleaving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Schedule {
    #[default]
    Parallel,
    Sequential,
}

/// Knobs controlling how a [`Runner`] schedules and bounds probes.
///
/// Construct with [`RunnerConfig::default`] and chain the builder methods to
/// override individual fields. Durations are best-effort. An in-flight probe
/// can overshoot the overall deadline by up to `attempt_timeout` because
/// running probes are not interrupted mid-attempt.
///
/// Retries use exponential backoff. Starting at `initial_interval`, each
/// failed attempt doubles the wait, clamped to `max_interval`. With `jitter`
/// enabled the actual wait is sampled uniformly from `[0, current]` to avoid
/// thundering-herd lockstep across coordinated restarts.
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
}

impl RunnerConfig {
    /// Default total wall-clock budget for a [`Runner::run`] call.
    ///
    /// Thirty seconds is enough for most container start-up readiness checks
    /// without leaving CI jobs stuck on a misconfigured target forever.
    pub const DEFAULT_OVERALL_TIMEOUT: Duration = Duration::from_secs(30);

    /// Default first retry interval after a failed probe.
    ///
    /// One hundred milliseconds is fast enough to catch local services that
    /// finish booting in under a second without hammering the target with
    /// hundreds of probes per second.
    pub const DEFAULT_INITIAL_INTERVAL: Duration = Duration::from_millis(100);

    /// Default upper bound on the exponential backoff between retries.
    ///
    /// Two seconds keeps the runner responsive on slow services without
    /// letting the wait grow into the tens of seconds where users start
    /// thinking the tool has hung.
    pub const DEFAULT_MAX_INTERVAL: Duration = Duration::from_secs(2);

    /// Default delay applied before the very first probe fires.
    ///
    /// Zero by default. Useful values are short delays that match a known
    /// minimum start-up cost on the target side, where probing earlier just
    /// wastes attempts.
    pub const DEFAULT_INITIAL_DELAY: Duration = Duration::ZERO;

    /// Default per-attempt timeout for one full probe.
    ///
    /// Bounds the time a single probe can spend on DNS, TCP, TLS, and the
    /// protocol roundtrip. Five seconds is the rough median for healthy
    /// services on local networks and cloud load balancers.
    pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

    /// Default consecutive-success threshold before a target is satisfied.
    ///
    /// One means the first ready probe wins. Higher values protect against
    /// flapping services that briefly report ready and then fall over again
    /// during their warm-up phase.
    pub const DEFAULT_SUCCESS_THRESHOLD: u32 = 1;

    /// Floor applied to interval and attempt-timeout knobs.
    ///
    /// A zero-millisecond interval would spin the retry loop without giving
    /// the OS a chance to schedule the next attempt. One millisecond is the
    /// smallest value that still lets the runtime breathe.
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
}

/// Drives a set of [`Target`] probes to completion under a single deadline.
///
/// Construct via [`Runner::new`] from a [`RunnerConfig`], then await
/// [`Runner::run`] with the list of targets and an optional event sink. The
/// `Runner` is consumed by `run` so a single instance cannot be reused.
///
/// The runner does not interrupt probes mid-attempt. The overall deadline
/// applies between attempts. Worst case overshoot is one `attempt_timeout`
/// past the deadline.
#[derive(Debug)]
#[non_exhaustive]
pub struct Runner {
    cfg: RunnerConfig,
}

/// Per-target slice of a [`Report`].
///
/// Holds the original input index so callers can correlate results back to
/// the order they passed in, even when parallel probes finished out of
/// order. `satisfied` already factors in the direction and the
/// success-threshold gate, so library code only needs to inspect this one
/// field to decide whether the target is done.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TargetReport {
    pub idx: usize,
    pub target: Target,
    pub attempts: u32,
    pub final_outcome: CheckOutcome,
    pub satisfied: bool,
}

/// Aggregate outcome of a [`Runner::run`] call.
///
/// Contains one [`TargetReport`] per input target, sorted by input order,
/// plus the total wall-clock time the run consumed. Use [`Report::all_ready`]
/// for a quick boolean answer or [`Report::assert_all_ready`] when you want
/// the run to surface a typed [`crate::Error::NotReady`] on partial success.
///
/// The report does not retain attempt-level events. Subscribe to the
/// [`EventSink`] passed into [`Runner::run`] if you need per-attempt data.
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

/// Event emitted by [`Runner::run`] over the optional [`EventSink`].
///
/// Workers emit events from spawned tasks, so consumers must drain the
/// receiver concurrently with `run` to avoid back-pressure stalling the
/// runner. The default channel is unbounded for now, see [`EventSink`].
///
/// `Attempt` fires after every probe attempt with the latency and immediate
/// ready bit. `Finished` fires once per target when its retry loop ends,
/// either because the target satisfied its readiness condition or because
/// the deadline elapsed.
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

/// Channel sender used to receive [`Event`]s during a run.
///
/// This is an unbounded sender. Slow consumers (a stalled terminal, a piped
/// JSON consumer that blocks on flush) will grow memory proportional to
/// attempt rate times target count. Drain promptly. A future major release
/// is expected to swap this for a bounded channel with a documented drop
/// policy.
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
