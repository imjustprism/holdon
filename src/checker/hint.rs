pub(crate) trait Hintable {
    fn hint(&self) -> Option<&'static str>;
}

#[allow(dead_code)]
pub(crate) mod hints {
    pub(crate) const TIMED_OUT: &str = "timed out";
    pub(crate) const SERVER_SLOW: &str = "server slow or unreachable";
    pub(crate) const NOT_LISTENING: &str = "service not listening on this port yet";
    pub(crate) const PORT_CLOSED: &str = "port closed or firewalled";
    pub(crate) const TCP_NO_BANNER: &str = "TCP connected but the server did not send any data before the attempt timeout, lower the timeout or drop ?expect-banner";
    pub(crate) const TCP_BANNER_MISMATCH: &str = "TCP banner did not contain the expected text, check the protocol greeting (SMTP 220, SSH-2.0, etc.) and the matcher";
    pub(crate) const NET_UNREACHABLE: &str = "network or route problem, not a port-closed issue";
    pub(crate) const PG_NOT_READY: &str = "server not accepting queries yet";
    pub(crate) const PG_TLS: &str =
        "connection failed before reaching the server (TLS may be required)";
    pub(crate) const PG_CREDS: &str = "check credentials in the connection URL";
    pub(crate) const PG_NO_DB: &str = "database does not exist (or still initializing)";
    pub(crate) const PG_STARTING: &str = "server is starting up or shutting down, keep retrying";
    pub(crate) const PG_RECOVERY: &str = "server is in recovery or read-only mode";
    pub(crate) const PG_TABLE_MISSING: &str =
        "expected table not found via information_schema.tables, check name and search_path";
    pub(crate) const REDIS_NOT_READY: &str = "server slow or not yet listening";
    pub(crate) const REDIS_AUTH: &str = "set the password in the URL or via AUTH";
    pub(crate) const REDIS_LOADING: &str = "redis is loading the dataset into memory";
    pub(crate) const REDIS_CLUSTER: &str = "cluster topology not yet stable";
    pub(crate) const REDIS_TLS: &str =
        "TLS handshake failed, check rediss:// scheme and server certificate";
    pub(crate) const REDIS_KEY_MISSING: &str =
        "expected key not present in redis, producer may not have written it yet";
    pub(crate) const REDIS_VALUE_MISMATCH: &str =
        "key was present but its value did not match the supplied matcher";
    pub(crate) const MYSQL_NOT_READY: &str = "server not accepting connections yet";
    pub(crate) const MYSQL_AUTH: &str = "check credentials in the connection URL";
    pub(crate) const MYSQL_NO_DB: &str =
        "database does not exist or user lacks access (still initializing?)";
    pub(crate) const MYSQL_TLS: &str =
        "TLS negotiation failed, pass ?ssl-mode=disable for plaintext or check the server cert";
    pub(crate) const MYSQL_HOST_BLOCKED: &str =
        "server blocked this host (too many connection errors), FLUSH HOSTS on the server";
    pub(crate) const MYSQL_TABLE_MISSING: &str =
        "expected table not found via information_schema.tables, check name and current database";
    pub(crate) const GRPC_NOT_SERVING: &str =
        "server reachable but reporting NOT_SERVING, app likely still warming up";
    pub(crate) const GRPC_UNIMPLEMENTED: &str =
        "server does not implement grpc.health.v1.Health, check the service is registered";
    pub(crate) const GRPC_SERVICE_UNKNOWN: &str =
        "server is up but does not know this service name, check the URL path";
    pub(crate) const GRPC_AUTH: &str = "missing or invalid credentials for the health endpoint";
    pub(crate) const GRPC_TLS: &str =
        "TLS handshake failed for grpcs://, verify server cert and SNI";
    pub(crate) const GRPC_UNAVAILABLE: &str = "server transient unavailable, will keep retrying";
    pub(crate) const GRPC_DEADLINE: &str =
        "server did not respond in time, raise --attempt-timeout";
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
    pub(crate) const HTTP_HEADER_ENCODING: &str = "response header contained non-ASCII bytes, the server is sending binary or non-UTF-8 data, not a regex mismatch";
    pub(crate) const HTTP_SLOW_RESPONSE: &str = "response was acceptable but exceeded the --max-rtt SLA, check upstream latency before promoting";
    pub(crate) const DNS_HINT: &str = "check hostname spelling and DNS server";
    pub(crate) const FILE_IO: &str = "permission or IO error reading the path";
    pub(crate) const EXEC_NOT_FOUND: &str =
        "executable not found in PATH or as relative/absolute path";
    pub(crate) const EXEC_PERMISSION: &str = "file exists but is not executable (chmod +x?)";
    pub(crate) const EXEC_NONZERO: &str = "command reported not-ready, will retry";
    pub(crate) const EXEC_TIMED_OUT: &str =
        "child did not finish before attempt timeout, increase --attempt-timeout";
    pub(crate) const LOG_NOT_YET: &str =
        "pattern not yet in log, app may still be starting, will keep checking";
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
    pub(crate) const CLEARTEXT_CREDS: &str = "credentials over a non-TLS scheme would be sent in plaintext, switch to the TLS scheme (rediss://, mongodb+srv:// or mongodb://?tls=true, kafkas://, amqps://) or remove the password";
    pub(crate) const DOCKER_NO_SOCKET: &str = "could not reach the Docker engine socket, check that the daemon is running and that DOCKER_HOST is set (or that the user has access to the default socket)";
    pub(crate) const DOCKER_NO_SUCH: &str = "container does not exist on this daemon, check the name and whether it has been created yet";
    pub(crate) const DOCKER_NOT_RUNNING: &str = "container exists but is not in the expected state, it may still be starting or have already exited";
    pub(crate) const DOCKER_NO_HEALTHCHECK: &str = "container has no HEALTHCHECK defined, drop ?healthy=true or add a HEALTHCHECK to the image";
    pub(crate) const DOCKER_UNHEALTHY: &str = "container reported unhealthy, inspect the most recent healthcheck logs with `docker inspect`";
    pub(crate) const DOCKER_PROTOCOL: &str = "unexpected response from the Docker engine API, ensure DOCKER_HOST points at a real Docker engine";
    pub(crate) const DOCKER_LOG_NOT_FOUND: &str = "container exists but its captured logs do not yet contain the expected text, app may still be starting";
    pub(crate) const DOCKER_LOG_TOO_LARGE: &str = "container's log tail exceeded the 2 MiB read cap, narrow the match string so it appears earlier or use a smaller tail";
    pub(crate) const K8S_NO_CONFIG: &str = "could not resolve a Kubernetes API server, run inside a pod with a service account or set KUBE_SERVER and KUBE_TOKEN";
    pub(crate) const K8S_API_UNREACHABLE: &str =
        "could not reach the Kubernetes API server, check connectivity and TLS";
    pub(crate) const K8S_AUTH: &str = "Kubernetes API rejected the bearer token, the service account or KUBE_TOKEN may be expired or lack RBAC permissions";
    pub(crate) const K8S_NOT_FOUND: &str =
        "resource does not exist in this namespace yet, the controller may not have created it";
    pub(crate) const K8S_NOT_READY: &str =
        "resource exists but is not yet reporting Ready/Complete";
    pub(crate) const K8S_PROTOCOL: &str =
        "unexpected response shape from the Kubernetes API server";
    pub(crate) const K8S_JOB_FAILED: &str = "job reported Failed=True, this will not recover by waiting, check pod logs and the BackoffLimit setting";
    pub(crate) const WS_NO_CONNECT: &str =
        "could not open TCP connection to the WebSocket endpoint, check host and port";
    pub(crate) const WS_HANDSHAKE: &str = "TCP connected but the WebSocket handshake failed, server may not speak the WebSocket protocol on this path";
    pub(crate) const WS_TLS: &str = "wss:// handshake failed, verify server cert and SNI";
    pub(crate) const WS_NO_MESSAGE: &str =
        "websocket connected but server closed the connection before sending a message";
    pub(crate) const WS_MESSAGE_MISMATCH: &str =
        "websocket received a message but it did not match the supplied matcher";
    pub(crate) const WS_BINARY_MESSAGE: &str =
        "websocket received a non-text frame, cannot match against ?expect-text or ?expect-regex";
    pub(crate) const PROCESS_NO_PID: &str =
        "no process with that pid is running yet, it may not have spawned";
    pub(crate) const PROCESS_NO_NAME: &str =
        "no process with that name is running yet, check spelling or wait for the launcher";
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
mod mysql_codes {
    pub(super) const ER_DBACCESS_DENIED: u16 = 1044;
    pub(super) const ER_ACCESS_DENIED: u16 = 1045;
    pub(super) const ER_BAD_DB: u16 = 1049;
    pub(super) const ER_HOST_IS_BLOCKED: u16 = 1129;
    pub(super) const ER_HOST_NOT_PRIVILEGED: u16 = 1130;
    pub(super) const ER_SERVER_SHUTDOWN: u16 = 1053;
    pub(super) const ER_NOT_SUPPORTED_AUTH_MODE: u16 = 1251;
}

#[cfg(feature = "mysql")]
impl Hintable for mysql_async::Error {
    fn hint(&self) -> Option<&'static str> {
        use mysql_async::IoError;
        use mysql_codes::{
            ER_ACCESS_DENIED, ER_BAD_DB, ER_DBACCESS_DENIED, ER_HOST_IS_BLOCKED,
            ER_HOST_NOT_PRIVILEGED, ER_NOT_SUPPORTED_AUTH_MODE, ER_SERVER_SHUTDOWN,
        };
        match self {
            Self::Server(e) => match e.code {
                ER_ACCESS_DENIED | ER_DBACCESS_DENIED | ER_NOT_SUPPORTED_AUTH_MODE => {
                    Some(hints::MYSQL_AUTH)
                }
                ER_BAD_DB => Some(hints::MYSQL_NO_DB),
                ER_HOST_IS_BLOCKED | ER_HOST_NOT_PRIVILEGED => Some(hints::MYSQL_HOST_BLOCKED),
                ER_SERVER_SHUTDOWN => Some(hints::MYSQL_NOT_READY),
                _ => None,
            },
            Self::Io(IoError::Io(e)) => match e.kind() {
                std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe => Some(hints::MYSQL_NOT_READY),
                _ => None,
            },
            Self::Io(IoError::Tls(_)) => Some(hints::MYSQL_TLS),
            Self::Driver(_) | Self::Other(_) | Self::Url(_) => None,
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

#[cfg(all(test, feature = "mysql"))]
mod mysql_hint_tests {
    use super::{Hintable, hints};

    const fn server_err(code: u16) -> mysql_async::Error {
        mysql_async::Error::Server(mysql_async::ServerError {
            code,
            message: String::new(),
            state: String::new(),
        })
    }

    #[test]
    fn access_denied_maps_to_auth_hint() {
        assert_eq!(server_err(1045).hint(), Some(hints::MYSQL_AUTH));
        assert_eq!(server_err(1044).hint(), Some(hints::MYSQL_AUTH));
        assert_eq!(server_err(1251).hint(), Some(hints::MYSQL_AUTH));
    }

    #[test]
    fn bad_db_maps_to_no_db_hint() {
        assert_eq!(server_err(1049).hint(), Some(hints::MYSQL_NO_DB));
    }

    #[test]
    fn host_blocked_maps_to_blocked_hint() {
        assert_eq!(server_err(1129).hint(), Some(hints::MYSQL_HOST_BLOCKED));
        assert_eq!(server_err(1130).hint(), Some(hints::MYSQL_HOST_BLOCKED));
    }

    #[test]
    fn server_shutdown_maps_to_not_ready() {
        assert_eq!(server_err(1053).hint(), Some(hints::MYSQL_NOT_READY));
    }

    #[test]
    fn unknown_server_code_has_no_hint() {
        assert_eq!(server_err(9999).hint(), None);
    }

    #[test]
    fn connection_refused_maps_to_not_ready() {
        let io = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let e = mysql_async::Error::Io(mysql_async::IoError::Io(io));
        assert_eq!(e.hint(), Some(hints::MYSQL_NOT_READY));
    }
}
