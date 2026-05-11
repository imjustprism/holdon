# holdon

[![crates.io](https://img.shields.io/crates/v/holdon.svg)](https://crates.io/crates/holdon)
[![docs.rs](https://img.shields.io/docsrs/holdon)](https://docs.rs/holdon)
[![CI](https://github.com/imjustprism/holdon/actions/workflows/ci.yml/badge.svg)](https://github.com/imjustprism/holdon/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/rust-1.85+-blue.svg)](https://blog.rust-lang.org/)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

> Wait for anything. Know why if it doesn't.

A next-gen "wait for service ready" CLI in Rust. One static binary, parallel
by default, protocol-aware, with diagnostic failures that actually tell you
what broke.

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

## Install

| Method            | Command                                                            |
| ----------------- | ------------------------------------------------------------------ |
| Cargo             | `cargo install holdon`                                             |
| Prebuilt binaries | [GitHub Releases](https://github.com/imjustprism/holdon/releases)  |
| Docker            | `docker pull ghcr.io/imjustprism/holdon`                           |
| Install script    | `curl -fsSL https://raw.githubusercontent.com/imjustprism/holdon/main/install.sh \| sh` |

Minimum supported Rust version: **1.85**.

## Shell completions and man page

```sh
holdon --generate-completion bash    > /etc/bash_completion.d/holdon
holdon --generate-completion zsh     > ~/.zsh/completions/_holdon
holdon --generate-completion fish    > ~/.config/fish/completions/holdon.fish
holdon --generate-completion power-shell  | iex
holdon --generate-manpage             > /usr/local/share/man/man1/holdon.1
```

Prebuilt copies of every shell's completion script plus the man page are attached to each [GitHub Release](https://github.com/imjustprism/holdon/releases) as `holdon-completions-and-manpage.tar.gz`.

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
```

Explicit CLI flags always win over the config file. See [`examples/holdon.toml`](https://github.com/imjustprism/holdon/tree/main/examples/holdon.toml).

## Quickstart

```sh
holdon :5432                              # wait for localhost:5432
holdon :5432 :6379 :3000                  # several ports in parallel
holdon :5432 -- npm run migrate           # exec a command once ready
holdon https://api.local/health -t 60s    # http with custom timeout
holdon postgres://user:pw@db/app          # postgres handshake
holdon exec:///usr/local/bin/check.sh     # custom readiness command
```

## Protocols

| Scheme                          | What it checks                            |
| ------------------------------- | ----------------------------------------- |
| `tcp://`, `:port`, `host:port`  | DNS resolve, TCP connect                  |
| `http://`, `https://`           | TCP, TLS, HTTP request (`-H`, `--method`, `--expect-body`, `--expect-body-regex`, `--expect-json`, `--no-follow-redirects`, `--ca-cert`, `--tls-min`) |
| `dns://`                        | Hostname resolves                         |
| `file:///path`                  | Path exists (`?mode=absent` inverse)      |
| `postgres://`, `postgresql://`  | Connect + `SELECT 1` (TLS by default)     |
| `mysql://`, `mariadb://`        | Connect + `SELECT 1` (TLS by default)     |
| `redis://`, `rediss://`         | Connect + `PING` (`rediss://` for TLS)    |
| `grpc://`, `grpcs://`           | `grpc.health.v1.Health/Check` unary (optional `/Service` path) |
| `influxdb://`, `influxdbs://`   | `/ping` (works for v1 and v2), optional `?expect-version=1\|2` |
| `log:///path?match=...`         | Wait for a substring or regex to appear in a local log file (last 1 MiB) |
| `exec://program?arg=...`        | External command, ready iff exit `0`      |

## Feature flags

Defaults (`http` + `json-output`) cover most CI use cases. Database probes
are opt-in to keep the default binary small.

| Feature         | Adds                                       |
| --------------- | ------------------------------------------ |
| `http`          | HTTP / HTTPS probes (rustls)               |
| `postgres`      | Postgres probe via `tokio-postgres` + rustls |
| `mysql`         | `MySQL` / `MariaDB` probe via `mysql_async` + rustls |
| `grpc`          | gRPC `Health/Check` probe via `tonic` + rustls |
| `influxdb`      | `InfluxDB` `/ping` probe (depends on `http`) |
| `redis`         | Redis probe via `redis` crate + rustls     |
| `json-output`   | `--output json` line-delimited events      |
| `all-databases` | `postgres` + `mysql` + `redis`             |
| `full`          | Everything above                           |

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

## Output modes

- **Plain** (default). Live spinner, colored status, sparklines on stderr.
  Auto-disabled in non-TTY environments.
- **JSON** (`--output json`). Line-delimited events on stdout, stable
  schema documented in [`docs/json-schema.md`](docs/json-schema.md).
- **Quiet** (`-q`). Only the exit code.

## Library

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

See the [examples directory](https://github.com/imjustprism/holdon/tree/main/examples)
and the [API docs](https://docs.rs/holdon).

## Security notes

- TLS is rustls only. No OpenSSL anywhere in the tree.
- Postgres and Redis use rustls with webpki roots out of the box.
  Postgres opportunistically upgrades to TLS unless the URL passes
  `?sslmode=disable`.
- HTTP redirects are followed up to 5 hops and refuse `https → http`
  downgrades.
- `--insecure` (HTTP only) disables TLS verification for HTTP probes and
  prints a stderr warning. Do not use in production.
- `exec://` runs whatever command you point it at. Treat target strings
  as code.
- Symlinks are not followed by `file://` probes.
- URL passwords are redacted in `Display`, `Debug`, and error chains.

See [SECURITY.md](SECURITY.md) for the full threat model and disclosure
instructions.

## License

Dual [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
