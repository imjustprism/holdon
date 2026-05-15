use std::fmt;
use std::time::Duration;

/// Result of one full probe attempt against a single target.
///
/// Carries the ordered list of stages the probe walked through, the total
/// wall-clock time the attempt consumed, and a final status that summarises
/// whether the target ended ready or failed.
///
/// A probe may produce multiple stages even when it succeeds. An HTTP target
/// typically emits DNS, TCP, TLS, and HTTP stages with per-stage latency.
/// Inspect the stages when you want to see where the time went or which step
/// actually broke.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckOutcome {
    pub stages: Vec<Stage>,
    pub total: Duration,
    pub status: Status,
}

/// Final readiness verdict attached to every [`CheckOutcome`].
///
/// `Ready` means every probe stage reported success. `Failed` covers any
/// stage error, timeout, or unexpected condition. The reverse-mode runner
/// inverts the verdict before reporting, so this enum always describes the
/// raw probe outcome rather than the operator-facing decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    Ready,
    Failed,
}

/// One step in a multi-stage probe.
///
/// A stage records what kind of work it represents, how long it took, and
/// whether it succeeded or produced a diagnostic error with an optional
/// operator-facing hint. Probes append stages in chronological order, so the
/// last stage is the one that decided the outcome.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Stage {
    pub kind: StageKind,
    pub took: Duration,
    pub result: StageResult,
}

/// Discriminator for the kind of work a [`Stage`] performed.
///
/// Each variant corresponds to a specific probe phase such as DNS lookup,
/// TCP connect, TLS handshake, or a protocol-specific roundtrip. The
/// machine-stable wire name for each variant is exposed via
/// [`StageKind::as_str`] and is part of the `--output json` schema.
///
/// The enum is `#[non_exhaustive]` so adding a new probe protocol does not
/// break downstream consumers. Match exhaustively where the project owns
/// every variant, and use a wildcard arm at external boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StageKind {
    Dns,
    Tcp,
    Http,
    File,
    Postgres,
    Redis,
    Mysql,
    Exec,
    Grpc,
    Log,
    Influxdb,
    Mongodb,
    Rabbitmq,
    Kafka,
    Temporal,
}

/// Outcome of a single [`Stage`].
///
/// The success variant carries no payload. The error variant carries a
/// formatted message describing what went wrong, plus an optional one-line
/// operator hint pointing at a likely fix. Hints are sourced from the
/// internal hint catalogue, so the same hint text appears across releases
/// for the same root cause.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StageResult {
    Ok,
    Err {
        message: Box<str>,
        hint: Option<Box<str>>,
    },
}

impl CheckOutcome {
    #[must_use]
    pub const fn ready(stages: Vec<Stage>, total: Duration) -> Self {
        Self {
            stages,
            total,
            status: Status::Ready,
        }
    }

    #[must_use]
    pub const fn failed(stages: Vec<Stage>, total: Duration) -> Self {
        Self {
            stages,
            total,
            status: Status::Failed,
        }
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready)
    }
}

impl StageKind {
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
            Self::Exec => "exec",
            Self::Grpc => "grpc",
            Self::Log => "log",
            Self::Influxdb => "influxdb",
            Self::Mongodb => "mongodb",
            Self::Rabbitmq => "rabbitmq",
            Self::Kafka => "kafka",
            Self::Temporal => "temporal",
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
            Self::Exec => "external command",
            Self::Grpc => "gRPC health",
            Self::Log => "log file match",
            Self::Influxdb => "InfluxDB ping",
            Self::Mongodb => "MongoDB ping",
            Self::Rabbitmq => "RabbitMQ AMQP",
            Self::Kafka => "Kafka metadata",
            Self::Temporal => "Temporal health",
        })
    }
}
