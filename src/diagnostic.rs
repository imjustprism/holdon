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
    Docker,
    K8s,
    Ws,
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
    pub const fn new(stages: Vec<Stage>, total: Duration, status: Status) -> Self {
        Self {
            stages,
            total,
            status,
        }
    }

    #[must_use]
    pub const fn ready(stages: Vec<Stage>, total: Duration) -> Self {
        Self::new(stages, total, Status::Ready)
    }

    #[must_use]
    pub const fn failed(stages: Vec<Stage>, total: Duration) -> Self {
        Self::new(stages, total, Status::Failed)
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, Status::Ready)
    }
}

impl StageKind {
    const fn info(self) -> (&'static str, &'static str) {
        match self {
            Self::Dns => ("dns", "DNS resolution"),
            Self::Tcp => ("tcp", "TCP connect"),
            Self::Http => ("http", "HTTP request"),
            Self::File => ("file", "filesystem"),
            Self::Postgres => ("postgres", "Postgres query"),
            Self::Redis => ("redis", "Redis PING"),
            Self::Mysql => ("mysql", "MySQL query"),
            Self::Exec => ("exec", "external command"),
            Self::Grpc => ("grpc", "gRPC health"),
            Self::Log => ("log", "log file match"),
            Self::Influxdb => ("influxdb", "InfluxDB ping"),
            Self::Mongodb => ("mongodb", "MongoDB ping"),
            Self::Rabbitmq => ("rabbitmq", "RabbitMQ AMQP"),
            Self::Kafka => ("kafka", "Kafka metadata"),
            Self::Temporal => ("temporal", "Temporal health"),
            Self::Docker => ("docker", "Docker container"),
            Self::K8s => ("k8s", "Kubernetes resource"),
            Self::Ws => ("ws", "WebSocket handshake"),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.info().0
    }
}

impl fmt::Display for StageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.info().1)
    }
}
