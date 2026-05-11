use std::fmt;
use std::time::Duration;

/// Result of one full probe attempt against a single target.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckOutcome {
    /// Ordered list of stages, earliest first.
    pub stages: Vec<Stage>,
    /// Total wall-clock time of this attempt, summed across all stages.
    pub total: Duration,
    /// Aggregate verdict, derived from whether the last stage succeeded.
    pub status: Status,
}

/// Whether a [`CheckOutcome`] represents a ready target or a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    /// All stages succeeded.
    Ready,
    /// At least one stage failed or timed out.
    Failed,
}

/// One step in a multi-stage probe.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Stage {
    /// Which kind of check this stage represents.
    pub kind: StageKind,
    /// Wall time spent in this stage.
    pub took: Duration,
    /// Pass-or-fail with optional human-readable details.
    pub result: StageResult,
}

/// Discriminator for the kind of work a [`Stage`] performed.
///
/// String names are stable across releases via [`StageKind::as_str`]. Adding
/// new variants is non-breaking thanks to `#[non_exhaustive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StageKind {
    /// Hostname resolution via the system resolver.
    Dns,
    /// TCP socket connection.
    Tcp,
    /// HTTP request and status check.
    Http,
    /// Filesystem existence check for `file://` targets.
    File,
    /// Postgres connect plus `SELECT 1`.
    Postgres,
    /// Redis connect plus `PING`.
    Redis,
    /// `MySQL` connect plus `SELECT 1`.
    Mysql,
    /// gRPC `Health/Check` unary call.
    Grpc,
    /// External command invocation for `exec://` targets.
    Exec,
}

/// Outcome of a single [`Stage`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StageResult {
    /// Stage succeeded.
    Ok,
    /// Stage failed, with a sanitized message and optional hint.
    Err {
        /// Sanitized error message with control bytes stripped and known
        /// secrets replaced with `***`.
        message: Box<str>,
        /// Optional one-line hint pointing at a likely fix.
        hint: Option<Box<str>>,
    },
}

impl CheckOutcome {
    /// Builds a [`CheckOutcome`] with [`Status::Ready`].
    #[must_use]
    pub const fn ready(stages: Vec<Stage>, total: Duration) -> Self {
        Self {
            stages,
            total,
            status: Status::Ready,
        }
    }

    /// Builds a [`CheckOutcome`] with [`Status::Failed`].
    #[must_use]
    pub const fn failed(stages: Vec<Stage>, total: Duration) -> Self {
        Self {
            stages,
            total,
            status: Status::Failed,
        }
    }

    /// Returns `true` if the outcome is [`Status::Ready`].
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready)
    }
}

impl StageKind {
    /// Returns the stable machine-readable identifier for this stage kind.
    ///
    /// Used in JSON output and structured logs. The string for an existing
    /// variant will not change.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Tcp => "tcp",
            Self::Http => "http",
            Self::File => "file",
            Self::Postgres => "postgres",
            Self::Redis => "redis",
            Self::Mysql => "mysql",
            Self::Grpc => "grpc",
            Self::Exec => "exec",
        }
    }
}

impl fmt::Display for StageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dns => "DNS resolution",
            Self::Tcp => "TCP connect",
            Self::Http => "HTTP request",
            Self::File => "filesystem",
            Self::Postgres => "Postgres query",
            Self::Redis => "Redis PING",
            Self::Mysql => "MySQL query",
            Self::Grpc => "gRPC health",
            Self::Exec => "external command",
        })
    }
}
