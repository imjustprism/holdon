<p align="center">
  <img src="assets/png/16-white-black.png" alt="holdon logo" width="160">
</p>

<h1 align="center">holdon</h1>

<p align="center">
  <strong>Wait for anything. Know why if it doesn't.</strong>
</p>

<p align="center">
  A next-gen "wait for service ready" CLI in Rust. One static binary, parallel by default, protocol-aware, with diagnostic failures that actually tell you what broke.
</p>

<p align="center">
  <a href="https://crates.io/crates/holdon"><img src="https://img.shields.io/crates/v/holdon.svg" alt="crates.io"></a>
  <a href="https://docs.rs/holdon"><img src="https://img.shields.io/docsrs/holdon" alt="docs.rs"></a>
  <a href="https://github.com/imjustprism/holdon/actions/workflows/ci.yml"><img src="https://github.com/imjustprism/holdon/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://blog.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.85+-blue.svg" alt="MSRV 1.85"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license"></a>
</p>

```text
$ holdon postgres://db:5432 redis://cache:6379 https://api/health
✓ ready postgres://db:5432 · 27ms
✓ ready redis://cache:6379 · 14ms
✗ failed https://api/health · 5.0s · ▁▂▄▆█ · 510ms
├ dns ✓ 2ms
├ tcp ✓ 3ms
└ http ✗ status 503
hint: service may still be initializing
→ 2/3 ready · 5.1s
```

## Why holdon

**Diagnostic stages, not "timed out".** Every probe is multi-stage (DNS, TCP, TLS, protocol). When a target fails you get the stage that broke and an operator-facing hint, not a stack trace.

**Parallel by default.** Pass a dozen targets in one command. They run concurrently. Sequential mode is opt-in via `--sequential`.

**Protocol-aware probes for 15 schemes.** TCP, HTTP, DNS, file, exec, log, Postgres, `MySQL`/`MariaDB`, Redis, `MongoDB`, `RabbitMQ` (AMQP), Kafka, Temporal, `InfluxDB` (v1/v2/v3), and gRPC `Health/Check`. Each probe speaks the real protocol instead of just opening a socket.

**Type-safe URL DSL.** `mongodb://`, `kafka://`, `temporal://`, etc. Query parameters validated at parse time. URL passwords and `?token=` values redacted in every error path, in `Display`, in `Debug`, and in CLI parse errors.

**One static binary.** musl build is under 4 MB with default features, under 1.5 MB with no defaults. No runtime, no shell-out, no OpenSSL anywhere in the dependency tree.

**Rustls everywhere.** Postgres, `MySQL`, Redis, `MongoDB`, `RabbitMQ`, Kafka, Temporal, HTTP, and gRPC all share one TLS stack with bundled webpki roots. No `native-tls`.

**Machine output.** `--output json` emits a stable line-delimited schema (`v: 1`) ready for `jq`. POSIX-aligned exit codes (`0`, `2`, `124`, `126`, `127`, `130`, `143`).

## Install

The recommended path is **cargo**:

```sh
cargo install holdon
```

Pick a feature set based on which probes you need:

```sh
cargo install holdon --no-default-features --features http,postgres
cargo install holdon --features all-databases
cargo install holdon --features full
```

Skip the compile step with [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall holdon
```

**Homebrew** (`macOS`, Linux):

```sh
brew install imjustprism/holdon/holdon
```

**Scoop** (Windows):

```powershell
scoop bucket add holdon https://github.com/imjustprism/scoop-holdon
scoop install holdon
```

Prebuilt binaries for Linux (gnu/musl, `x86_64` + `aarch64`), `macOS` (`x86_64` + `arm64`), and Windows ship with every release:

```sh
curl -fsSL https://raw.githubusercontent.com/imjustprism/holdon/main/install.sh | sh
```

Or grab a tarball from [GitHub Releases](https://github.com/imjustprism/holdon/releases).

A multi-arch Docker image is published to the GitHub Container Registry:

```sh
docker pull ghcr.io/imjustprism/holdon
docker run --rm ghcr.io/imjustprism/holdon tcp://db:5432
```

Verify the install:

```sh
holdon --version
```

Minimum supported Rust version: **1.85**.

## Quickstart

```sh
holdon :5432                              # wait for localhost:5432
holdon :5432 :6379 :3000                  # several ports in parallel
holdon :5432 -- npm run migrate           # exec a command once ready
holdon https://api.local/health -t 60s    # http with custom timeout
holdon postgres://user:pw@db/app          # postgres handshake
holdon exec:///usr/local/bin/check.sh     # custom readiness command
```

The argument after `--` is the command to run once every target is ready. holdon execs it directly (no shell), so quoting and signals work the same as `timeout(1)` or `kubectl exec`.

## Protocols

| Scheme                          | What it checks                            |
| ------------------------------- | ----------------------------------------- |
| `tcp://`, `:port`, `host:port`  | DNS resolve, TCP connect. Optional `?expect-banner=NEEDLE` or `?expect-banner-regex=PATTERN` reads the first 4 KiB after connect and matches against it (SMTP `220`, `SSH-2.0`, etc.) |
| `http://`, `https://`           | TCP, TLS, HTTP request (`-H`, `--method`, `--data`, `--expect-body`, `--expect-body-regex`, `--expect-json`, `--expect-header`, `--no-follow-redirects`, `--max-redirects`, `--ca-cert`, `--client-cert` + `--client-key`, `--tls-min`) |
| `dns://`                        | Hostname resolves. Optional `?expect-ip=1.2.3.4` (or IPv6) waits until that IP appears in the resolver's answer (DNS propagation check) |
| `file:///path`                  | Path exists (`?mode=absent` inverse)      |
| `postgres://`, `postgresql://`  | Connect + `SELECT 1` (TLS by default). Optional `?table=NAME` verifies a table exists in the session's current search path via parameterized `information_schema.tables`. |
| `mysql://`, `mariadb://`        | Connect + `SELECT 1` (TLS by default). Optional `?table=NAME` verifies a table exists in the database named in the URL via parameterized `information_schema.tables`. |
| `redis://`, `rediss://`         | Connect + `PING` (`rediss://` for TLS). Optional `?key=NAME` requires the key to exist. Optional `?match=NEEDLE` or `?regex=PATTERN` asserts the value contains a substring or matches a regex. |
| `grpc://`, `grpcs://`           | `grpc.health.v1.Health/Check` unary (optional `/Service` path) |
| `influxdb://`, `influxdbs://`   | `/ping` for v1, v2, v3. Optional `?expect-version=1\|2\|3` and `?token=...` (Bearer/Token auth for v3 OSS) |
| `mongodb://`, `mongodb+srv://`  | Connect + admin `ping` command (SRV-aware) |
| `amqp://`, `amqps://`           | `RabbitMQ` AMQP connect, optional `?queue=` / `?exchange=` passive declare |
| `kafka://`, `kafkas://`         | Kafka broker Metadata fetch, optional `?topic=` and `?expect-partitions=` |
| `temporal://`, `temporals://`   | Temporal server gRPC `Health/Check` on `WorkflowService` |
| `log:///path?match=...`         | Wait for a substring or regex to appear in a local log file (last 1 MiB) |
| `exec://program?arg=...`        | External command, ready iff exit `0`      |
| `process://<pid>`, `process://<name>` | Process exists by PID or by executable name (Windows strips `.exe`) |
| `ws://`, `wss://`               | WebSocket handshake. Optional `?expect-text=NEEDLE` or `?expect-regex=PATTERN` waits for the first frame and matches against it |
| `docker://name`                 | Docker container `?state=running\|paused\|exited\|...`, `?healthy=true`, plus `?log-match=NEEDLE` or `?log-regex=PATTERN` against the last 200 log lines |
| `k8s://<kind>/<ns>/<name>`      | Kubernetes pod, deployment, or job. Optional `?condition=Ready,Initialized` requires every listed condition type to report `True` (overrides the default per-kind readiness rule) |

## Feature flags

Defaults (`http` + `json-output`) cover most CI use cases. Database and message-broker probes are opt-in to keep the default binary small.

| Feature         | Adds                                       |
| --------------- | ------------------------------------------ |
| `http`          | HTTP / HTTPS probes (rustls)               |
| `postgres`      | Postgres probe via `tokio-postgres` + rustls |
| `mysql`         | `MySQL` / `MariaDB` probe via `mysql_async` + rustls |
| `redis`         | Redis probe via `redis` crate + rustls     |
| `mongodb`       | `MongoDB` probe via `mongodb` driver + rustls (SRV-aware) |
| `rabbitmq`      | `RabbitMQ` AMQP probe via `lapin` + rustls (optional queue/exchange check) |
| `kafka`         | Kafka Metadata probe via pure-Rust `rskafka` + rustls (optional topic/partition check) |
| `temporal`      | Temporal server gRPC `Health/Check` probe (depends on `grpc`) |
| `influxdb`      | `InfluxDB` `/ping` probe (depends on `http`) |
| `grpc`          | gRPC `Health/Check` probe via `tonic` + rustls |
| `json-output`   | `--output json` line-delimited events      |
| `process`       | `process://<pid\|name>` readiness via `sysinfo` |
| `all-databases` | `postgres` + `mysql` + `redis` + `mongodb` |
| `full`          | Everything above                           |

## Config file

Pass `--config holdon.toml`, or drop `holdon.toml` / `.holdon.toml` next to where you run `holdon` and it's auto-detected.

```toml
interval = "200ms"
timeout = "60s"
success_threshold = 2

targets = [
  "tcp://db:5432",
  "https://api.local/health",
]

[[check]]
name = "slow database"
target = "postgres://db:5432"
interval = "1s"
attempt_timeout = "15s"
success_threshold = 3
```

Per-`[[check]]` `interval`, `attempt_timeout`, and `success_threshold` override the global value for that one target. Omitted fields inherit the global setting.

Explicit CLI flags always win over the config file. See [`examples/holdon.toml`](https://github.com/imjustprism/holdon/tree/main/examples/holdon.toml).

`--log-file PATH` (or `HOLDON_LOG_FILE=PATH`) appends one JSON event per line to a file in addition to the terminal output. Each line carries `v`, `ts_unix_ms`, and an `event` field (`start`, `attempt`, `target`, or `end`). The file is opened in append mode so multiple runs accumulate.

## Recipes

### Docker Compose

Block an app container until its dependencies are reachable. Mount the static binary or use the published image as an init step.

```yaml
services:
  app:
    image: my-app
    depends_on: [db, cache, queue]
    entrypoint: ["/usr/local/bin/holdon"]
    command:
      - postgres://app:secret@db:5432
      - redis://cache:6379
      - amqp://queue:5672
      - --timeout=60s
      - --
      - /app/start.sh
    volumes:
      - ./holdon:/usr/local/bin/holdon:ro

  db: { image: postgres:16 }
  cache: { image: redis:7 }
  queue: { image: rabbitmq:3 }
```

The argument after `--` runs once every target is ready. Exits non-zero if any target misses the deadline, so Compose marks the service unhealthy.

### Kubernetes initContainer

```yaml
spec:
  initContainers:
    - name: wait-for-deps
      image: ghcr.io/imjustprism/holdon:latest
      args:
        - postgres://app:$(DB_PASSWORD)@db.default.svc:5432
        - https://auth.default.svc/healthz
        - kafka://broker.default.svc:9092
        - --timeout=120s
      env:
        - name: DB_PASSWORD
          valueFrom: { secretKeyRef: { name: db, key: password } }
  containers:
    - name: app
      image: my-app
```

Any non-zero exit from an initContainer triggers a restart per the pod's `restartPolicy`. Use `--timeout-exit-code=<N>` only when a surrounding controller distinguishes between exit codes, otherwise the default `124` is fine.

### GitHub Actions

Wait for service containers before running integration tests.

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    services:
      postgres: { image: postgres:16, ports: ["5432:5432"], env: { POSTGRES_PASSWORD: pw } }
      redis: { image: redis:7, ports: ["6379:6379"] }
    steps:
      - uses: actions/checkout@v4
      - uses: cargo-bins/cargo-binstall@v1.19.1
      - run: cargo binstall -y holdon
      - run: holdon :5432 :6379 --timeout=30s
      - run: cargo test
```

### justfile / Makefile

```just
wait-deps:
    holdon postgres://localhost:5432 redis://localhost:6379 \
           https://api.local/health \
           --timeout=60s --success-threshold=2

dev: wait-deps
    cargo run
```

```makefile
.PHONY: wait-deps dev
wait-deps:
	holdon postgres://localhost:5432 redis://localhost:6379 --timeout=60s

dev: wait-deps
	cargo run
```

### CI teardown (reverse mode)

Block on a port becoming free, a stale lock file vanishing, or a deployment finishing draining.

```sh
holdon :5432 --reverse --timeout=30s        # wait for port to close
holdon file:///var/run/app.pid --reverse    # wait for pidfile to disappear
holdon https://app/health --reverse         # wait for service to go down
```

### JSON output to jq

```sh
holdon postgres://db:5432 https://api/health --output json --timeout=30s \
  | jq -c 'select(.event == "target") | {target, satisfied, attempts}'
```

Schema documented in [`docs/json-schema.md`](docs/json-schema.md). `v: 1` is stable; adding fields is non-breaking.

### Retry tuning

Defaults: 100ms initial, exponential doubling, 2s cap, jitter on. Override per scenario.

```sh
holdon https://slow-cold-start/health \
  --interval=1s --max-interval=10s --timeout=5m

holdon :5432 --no-jitter --interval=250ms     # deterministic scheduling
holdon :5432 --success-threshold=3            # protect against flapping
holdon :5432 --initial-delay=2s               # give the service a head start
```

### Mutual TLS (HTTPS)

Send a client certificate and key for mutual TLS handshakes. PEM only; both flags must be set together.

```sh
holdon https://api.local/health \
  --ca-cert     ./ca.pem        \
  --client-cert ./client.pem    \
  --client-key  ./client.key
```

Env-bindable: `HOLDON_CLIENT_CERT`, `HOLDON_CLIENT_KEY`. Invalid PEM is reported on stderr and the probe falls back to no client auth.

### Header assertions

Repeatable. Each `--expect-header NAME=REGEX` must match a header in the response.

```sh
holdon https://api/health \
  --expect-status 200 \
  --expect-header 'content-type=^application/json' \
  --expect-header 'x-app-ready=^true$'
```

Failure hints: `HTTP_HEADER_MISSING` (header absent), `HTTP_HEADER_MISMATCH` (regex did not match the value), `HTTP_HEADER_ENCODING` (header contained non-ASCII bytes; server is sending binary or non-UTF-8 data).

### Environment variables

Every flag has a `HOLDON_*` env var for container-friendly configuration.

```sh
HOLDON_TIMEOUT=60s HOLDON_INTERVAL=500ms HOLDON_OUTPUT=json \
  holdon postgres://db:5432
```

## Output modes

- **Plain** (default). Live spinner, colored status, sparklines on stderr. Auto-disabled in non-TTY environments and when `NO_COLOR` is set.
- **JSON** (`--output json`). Line-delimited events on stdout, stable schema documented in [`docs/json-schema.md`](docs/json-schema.md). Versioned (`v: 1`). Adding fields is non-breaking, removing or renaming is.
- **Quiet** (`-q`). Only the exit code.

## Exit codes

| Code  | Meaning                                             |
| ----- | --------------------------------------------------- |
| `0`   | All targets ready                                   |
| `2`   | CLI misuse or parse error                           |
| `124` | Overall timeout elapsed (GNU `timeout` convention)  |
| `126` | Exec'd child not executable                         |
| `127` | Exec'd child binary not found                       |
| `130` | Interrupted by SIGINT (Ctrl-C)                      |
| `143` | Interrupted by SIGTERM                              |

Override the timeout exit code with `--timeout-exit-code <N>` when wrapping in Docker/Kubernetes lifecycle hooks that expect a specific code.

## Shell completions and man page

```sh
holdon --generate-completion bash          > /etc/bash_completion.d/holdon
holdon --generate-completion zsh           > ~/.zsh/completions/_holdon
holdon --generate-completion fish          > ~/.config/fish/completions/holdon.fish
holdon --generate-completion power-shell   | iex
holdon --generate-manpage                  > /usr/local/share/man/man1/holdon.1
```

Prebuilt completions for every shell plus the man page are attached to each [release](https://github.com/imjustprism/holdon/releases) as `holdon-completions-and-manpage.tar.gz`.

## Library

holdon is also a Rust crate. The same probe engine is exposed through `Runner` and `Target`:

```rust,no_run
use std::time::Duration;
use holdon::{Runner, Target};
use holdon::runner::RunnerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let targets = vec![
        "postgres-host:5432".parse::<Target>()?,
        "redis-host:6379".parse::<Target>()?,
    ];
    let cfg = RunnerConfig::default().timeout(Duration::from_secs(30));
    let report = Runner::new(cfg).run(targets, None).await;
    report.assert_all_ready()?;
    Ok(())
}
```

See the [examples directory](https://github.com/imjustprism/holdon/tree/main/examples) and the [API docs](https://docs.rs/holdon).

## Security

- **TLS is rustls only.** No OpenSSL anywhere in the tree. `cargo-deny` blocks it.
- **Rustls everywhere.** Every TLS-capable probe (HTTP, Postgres, `MySQL`, Redis, `MongoDB`, `RabbitMQ`, Kafka, Temporal, gRPC) uses the same ring-backed rustls stack with bundled webpki roots.
- **Password redaction.** URL passwords are stripped in `Display`, `Debug`, and every error path. Same for `?token=` query values on schemes that accept them.
- **Parse errors scrub secrets.** CLI errors like "invalid target ..." percent-decode query keys before matching, so `?to%6Bken=...` cannot bypass the redaction.
- **HTTP redirect policy.** Followed up to 5 hops. `https → http` downgrades refused.
- **`--insecure` is HTTP-only.** Prints a stderr warning on every run. Do not use in production.
- **`exec://` runs whatever you point it at.** Treat target strings as code at the invocation site.
- **`file://` and `log://` use `symlink_metadata`.** Symlinks are not followed into attacker-controlled paths.
- **No telemetry.** No phone-home, no analytics, ever.

See [SECURITY.md](SECURITY.md) for the full threat model and disclosure instructions.

## Contributing

Bug reports, feature requests, and PRs are welcome.

- Branch naming: `feat/<short-name>`, `fix/<short-name>`, `docs/<short-name>`, `chore/<short-name>`.
- Run `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` before opening a PR.
- New probes follow the `src/checker/<name>.rs` shape: a `pub(super) async fn probe(...)` returning `Vec<Stage>` plus a feature gate in `Cargo.toml`.

## Star History

<a href="https://star-history.com/#imjustprism/holdon&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=imjustprism/holdon&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=imjustprism/holdon&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=imjustprism/holdon&type=Date" />
  </picture>
</a>

## Contributors

[![Contributors](https://contrib.rocks/image?repo=imjustprism/holdon)](https://github.com/imjustprism/holdon/graphs/contributors)

## License

Dual [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
