use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckOutcome {
    pub stages: Vec<Stage>,
    pub total: Duration,
    pub status: Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Stage {
    pub kind: StageKind,
    pub took: Duration,
    pub result: StageResult,
}

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
    Process,
}

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
            Self::Process => ("process", "Process readiness"),
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
