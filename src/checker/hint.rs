pub(crate) trait Hintable {
    fn hint(&self) -> Option<&'static str>;
}

#[allow(dead_code)]
pub(crate) mod hints {
    pub(crate) const TIMED_OUT: &str = "timed out";
    pub(crate) const SERVER_SLOW: &str = "server slow or unreachable";
    pub(crate) const NOT_LISTENING: &str = "service not listening on this port yet";
    pub(crate) const PORT_CLOSED: &str = "port closed or firewalled";
    pub(crate) const NET_UNREACHABLE: &str = "network or route problem, not a port-closed issue";
    pub(crate) const PG_NOT_READY: &str = "server not accepting queries yet";
    pub(crate) const PG_TLS: &str =
        "connection failed before reaching the server (TLS may be required)";
    pub(crate) const PG_CREDS: &str = "check credentials in the connection URL";
    pub(crate) const PG_NO_DB: &str = "database does not exist (or still initializing)";
    pub(crate) const PG_STARTING: &str = "server is starting up or shutting down, keep retrying";
    pub(crate) const PG_RECOVERY: &str = "server is in recovery or read-only mode";
    pub(crate) const PG_TABLE_MISSING: &str =
        "expected table not found via information_schema.tables; check name and search_path";
    pub(crate) const REDIS_NOT_READY: &str = "server slow or not yet listening";
    pub(crate) const REDIS_AUTH: &str = "set the password in the URL or via AUTH";
    pub(crate) const REDIS_LOADING: &str = "redis is loading the dataset into memory";
    pub(crate) const REDIS_CLUSTER: &str = "cluster topology not yet stable";
    pub(crate) const REDIS_TLS: &str =
        "TLS handshake failed, check rediss:// scheme and server certificate";
    pub(crate) const REDIS_KEY_MISSING: &str =
        "expected key not present in redis; producer may not have written it yet";
    pub(crate) const REDIS_VALUE_MISMATCH: &str =
        "key was present but its value did not match the supplied matcher";
    pub(crate) const MYSQL_NOT_READY: &str = "server not accepting connections yet";
    pub(crate) const MYSQL_AUTH: &str = "check credentials in the connection URL";
    pub(crate) const MYSQL_NO_DB: &str =
        "database does not exist or user lacks access (still initializing?)";
    pub(crate) const MYSQL_TLS: &str =
        "TLS negotiation failed, pass ?ssl-mode=disable for plaintext or check the server cert";
    pub(crate) const MYSQL_HOST_BLOCKED: &str =
        "server blocked this host (too many connection errors); FLUSH HOSTS on the server";
    pub(crate) const MYSQL_TABLE_MISSING: &str =
        "expected table not found via information_schema.tables; check name and current database";
    pub(crate) const GRPC_NOT_SERVING: &str =
        "server reachable but reporting NOT_SERVING; app likely still warming up";
    pub(crate) const GRPC_UNIMPLEMENTED: &str =
        "server does not implement grpc.health.v1.Health; check the service is registered";
    pub(crate) const GRPC_SERVICE_UNKNOWN: &str =
        "server is up but does not know this service name; check the URL path";
    pub(crate) const GRPC_AUTH: &str = "missing or invalid credentials for the health endpoint";
    pub(crate) const GRPC_TLS: &str =
        "TLS handshake failed for grpcs://; verify server cert and SNI";
    pub(crate) const GRPC_UNAVAILABLE: &str = "server transient unavailable; will keep retrying";
    pub(crate) const GRPC_DEADLINE: &str =
        "server did not respond in time; raise --attempt-timeout";
    pub(crate) const HTTP_RETRY: &str = "service may still be initializing";
    pub(crate) const HTTP_BODY_MISMATCH: &str =
        "response status was acceptable but the body did not match --expect-body";
    pub(crate) const HTTP_BODY_REGEX_MISMATCH: &str =
        "response status was acceptable but the body did not match --expect-body-regex";
    pub(crate) const HTTP_JSON_MISMATCH: &str =
        "JSON body shape or value did not match --expect-json";
    pub(crate) const HTTP_HEADER_MISSING: &str =
        "expected response header was not present, check the server response";
    pub(crate) const HTTP_HEADER_MISMATCH: &str =
        "response header was present but did not match the --expect-header regex";
    pub(crate) const HTTP_HEADER_ENCODING: &str = "response header contained non-ASCII bytes; the server is sending binary or non-UTF-8 data, not a regex mismatch";
    pub(crate) const DNS_HINT: &str = "check hostname spelling and DNS server";
    pub(crate) const FILE_IO: &str = "permission or IO error reading the path";
    pub(crate) const EXEC_NOT_FOUND: &str =
        "executable not found in PATH or as relative/absolute path";
    pub(crate) const EXEC_PERMISSION: &str = "file exists but is not executable (chmod +x?)";
    pub(crate) const EXEC_NONZERO: &str = "command reported not-ready, will retry";
    pub(crate) const EXEC_TIMED_OUT: &str =
        "child did not finish before attempt timeout, increase --attempt-timeout";
    pub(crate) const LOG_NOT_YET: &str =
        "pattern not yet in log, app may still be starting; will keep checking";
    pub(crate) const LOG_PATH: &str =
        "log file is missing, check the path and that the producer has started writing";
    pub(crate) const INFLUXDB_NOT_READY: &str =
        "influxdb server slow or not yet listening on the ping endpoint";
    pub(crate) const INFLUXDB_VERSION: &str =
        "server major version did not match expect-version, check the target server";
    pub(crate) const INFLUXDB_PARSE: &str =
        "fix the influxdb:// URL, only ?expect-version=1|2|3 and ?token=... are supported";
    pub(crate) const INFLUXDB_AUTH: &str =
        "/ping returned 401, send ?token=... for v3 OSS or start the server with --without-auth";
    pub(crate) const MONGODB_NOT_READY: &str =
        "mongodb server slow, not yet accepting connections, or unreachable";
    pub(crate) const MONGODB_AUTH: &str =
        "auth failed, check username, password, and authSource in the URL";
    pub(crate) const MONGODB_NO_PRIMARY: &str =
        "no primary available, the replica set is electing or unreachable";
    pub(crate) const MONGODB_TLS: &str =
        "tls handshake failed, check ?tls=true and CA cert configuration";
    pub(crate) const RABBITMQ_NOT_READY: &str =
        "rabbitmq broker slow, not yet listening, or unreachable";
    pub(crate) const RABBITMQ_AUTH: &str = "auth failed, check username and password in the URL";
    pub(crate) const RABBITMQ_VHOST: &str =
        "vhost denied, check the /vhost path in the URL and broker permissions";
    pub(crate) const RABBITMQ_QUEUE: &str =
        "queue or exchange does not exist on the broker, check name and vhost";
    pub(crate) const RABBITMQ_TLS: &str =
        "tls handshake failed, check amqps:// and CA cert configuration";
    pub(crate) const KAFKA_NOT_READY: &str =
        "kafka broker slow, not yet listening, or controller not elected";
    pub(crate) const KAFKA_TOPIC_MISSING: &str =
        "topic not found in broker metadata, check name or autocreate setting";
    pub(crate) const KAFKA_PARTITION_COUNT: &str =
        "topic has fewer partitions than required by ?expect-partitions";
    pub(crate) const KAFKA_TLS: &str =
        "tls handshake failed, check kafkas:// and CA cert configuration";
}

impl Hintable for std::io::Error {
    fn hint(&self) -> Option<&'static str> {
        match self.kind() {
            std::io::ErrorKind::ConnectionRefused => Some(hints::NOT_LISTENING),
            std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable => {
                Some(hints::NET_UNREACHABLE)
            }
            std::io::ErrorKind::TimedOut => Some(hints::PORT_CLOSED),
            std::io::ErrorKind::PermissionDenied => Some(hints::FILE_IO),
            _ => None,
        }
    }
}

#[cfg(feature = "http")]
impl Hintable for reqwest::Error {
    fn hint(&self) -> Option<&'static str> {
        if self.is_timeout() {
            Some(hints::SERVER_SLOW)
        } else if self.is_connect() {
            Some(hints::PORT_CLOSED)
        } else if self.is_status() {
            Some(hints::HTTP_RETRY)
        } else {
            None
        }
    }
}

#[cfg(feature = "postgres")]
impl Hintable for tokio_postgres::Error {
    fn hint(&self) -> Option<&'static str> {
        use tokio_postgres::error::SqlState;
        let Some(code) = self.code() else {
            return Some(hints::PG_TLS);
        };
        if code == &SqlState::INVALID_PASSWORD
            || code == &SqlState::INVALID_AUTHORIZATION_SPECIFICATION
        {
            return Some(hints::PG_CREDS);
        }
        if code == &SqlState::INVALID_CATALOG_NAME {
            return Some(hints::PG_NO_DB);
        }
        if code == &SqlState::CANNOT_CONNECT_NOW
            || code == &SqlState::ADMIN_SHUTDOWN
            || code == &SqlState::CRASH_SHUTDOWN
        {
            return Some(hints::PG_STARTING);
        }
        if code == &SqlState::READ_ONLY_SQL_TRANSACTION {
            return Some(hints::PG_RECOVERY);
        }
        None
    }
}

#[cfg(feature = "mysql")]
impl Hintable for mysql_async::Error {
    fn hint(&self) -> Option<&'static str> {
        let s = self.to_string().to_ascii_lowercase();
        if s.contains("access denied") || s.contains("authentication") {
            Some(hints::MYSQL_AUTH)
        } else if s.contains("unknown database") {
            Some(hints::MYSQL_NO_DB)
        } else if s.contains("ssl") || s.contains("tls") || s.contains("certificate") {
            Some(hints::MYSQL_TLS)
        } else if s.contains("host") && s.contains("blocked") {
            Some(hints::MYSQL_HOST_BLOCKED)
        } else if s.contains("connection refused") || s.contains("server has gone away") {
            Some(hints::MYSQL_NOT_READY)
        } else {
            None
        }
    }
}

#[cfg(feature = "redis")]
impl Hintable for redis::RedisError {
    fn hint(&self) -> Option<&'static str> {
        use redis::ErrorKind::{
            AuthenticationFailed, BusyLoadingError, ClusterDown, InvalidClientConfig, MasterDown,
        };
        match self.kind() {
            AuthenticationFailed => Some(hints::REDIS_AUTH),
            BusyLoadingError => Some(hints::REDIS_LOADING),
            MasterDown | ClusterDown => Some(hints::REDIS_CLUSTER),
            InvalidClientConfig => Some(hints::REDIS_TLS),
            _ => None,
        }
    }
}
