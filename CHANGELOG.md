# Changelog

All notable changes to this project will be documented in this file. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). This
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `redis://...?key=NAME` runs `GET NAME` after `PING`. Probe fails if the
  key is absent. Optional `?match=NEEDLE` or `?regex=PATTERN` further
  asserts the value contains the substring or matches the regex. Mutually
  exclusive, both require `?key`.
- Hints `REDIS_KEY_MISSING` and `REDIS_VALUE_MISMATCH`.
- Public `RedisKeyExpect` re-exported from the crate root.

### Changed

- `Target::Redis` variant is now `#[non_exhaustive]` and gains
  `expect_key: Option<RedisKeyExpect>`. Pattern matches on the prior
  `{ url }` shape need `{ url, .. }`.

## [0.2.1] - 2026-05-12

### Added

- `cargo binstall holdon` downloads the prebuilt GitHub Release binary
  instead of compiling. `[package.metadata.binstall]` points at the
  tag's tarball or `.zip` (Windows) with the correct bin path.
- HTTP probes accept `--data BODY` to send a request body. Defaults to
  `Content-Type: application/octet-stream` unless overridden via `-H`.
- `--timeout`, `--interval`, and related duration flags accept the
  literal strings `never`, `infinite`, and `none` (case-insensitive) as
  "wait indefinitely" sentinels mapping to `Duration::MAX`.
- Docker images now built for `linux/amd64` AND `linux/arm64`. Single
  multi-arch manifest published to `ghcr.io/imjustprism/holdon`.
- Distribution channels live: Homebrew tap
  (`brew install imjustprism/holdon/holdon`), Scoop bucket
  (`scoop bucket add holdon https://github.com/imjustprism/scoop-holdon`),
  Winget auto-submission on each tag.

### Changed

- README install section advertises `cargo binstall`, Homebrew, and
  Scoop alongside the existing prebuilt-binaries path.

### Fixed

- Bare `0` keeps zero-duration semantics so env vars like
  `HOLDON_INTERVAL=0` continue to mean "no delay" instead of being
  silently reinterpreted as the unlimited sentinel.

## [0.2.0] - 2026-05-12

### Added

- `influxdb://` and `influxdbs://` probe. Hits `/ping` for InfluxDB v1,
  v2, and v3. Optional `?expect-version=1|2|3` checks the
  `X-Influxdb-Version` (v1/v2) or `X-Influxdb-Build` (v3) response
  header. Optional `?token=...` sends `Authorization: Token ...` so the
  probe works against v3 OSS servers that require auth on `/ping` by
  default. 401 responses route to a dedicated auth hint. Token query
  value redacted in `Display`, in CLI parse errors, and in every error
  path. Behind the new `influxdb` cargo feature (depends on `http`).
  Bundled in `full`.
- `mongodb://` and `mongodb+srv://` probe. Parses the connection string
  via the official `mongodb` driver, runs admin `{ ping: 1 }`. SRV
  discovery via DNS, TLS through `?tls=true`. Operator hints for auth
  failure, no-primary, and TLS handshake errors. Password redacted in
  `Display`, `Debug`, and every error path. Behind the new `mongodb`
  cargo feature. Included in `all-databases` and `full`.
- `amqp://` and `amqps://` probe for `RabbitMQ` and any AMQP 0-9-1
  broker via `lapin` with rustls. Optional `?queue=NAME` performs a
  passive queue declare and fails if the queue is absent. Optional
  `?exchange=NAME` performs a passive exchange declare. Operator hints
  classify access-refused, vhost-denied, queue-not-found, and TLS
  handshake errors. Password redacted in `Display`, `Debug`, and every
  error path. Behind the new `rabbitmq` cargo feature.
- `kafka://` and `kafkas://` probe via pure-Rust `rskafka` with rustls.
  Fetches the Metadata API to verify the broker is reachable and
  serving requests. Optional `?topic=NAME` requires the topic to exist;
  optional `?expect-partitions=N` requires the named topic to have at
  least N partitions. Hints distinguish topic-missing, partition
  shortfall, TLS handshake, and generic not-ready failures. Behind the
  new `kafka` cargo feature.
- `temporal://` and `temporals://` probe. Issues a gRPC
  `grpc.health.v1.Health/Check` against
  `temporal.api.workflowservice.v1.WorkflowService`. TLS via rustls for
  `temporals://`. Reuses every gRPC operator hint (`NOT_SERVING`,
  `UNIMPLEMENTED`, `UNAVAILABLE`, TLS handshake, auth, deadline).
  Behind the new `temporal` cargo feature (depends on `grpc`).

### Changed

- `all-databases` feature now includes `mongodb`.
- `full` feature bundles `mongodb`, `rabbitmq`, `kafka`, `temporal`,
  `influxdb` alongside the existing probe set.
- README restyled with logo header, value-prop section, security and
  contributors sections, and a 15-scheme protocol table.
- Shared `install_rustls_provider_once` helper consolidates the
  per-module `OnceLock` provider installer across `mysql`, `mongodb`,
  `rabbitmq`, and `kafka`.

### Fixed

- MongoDB probe no longer races the outer `tokio::time::timeout` with the
  driver's 30-second `server_selection_timeout` default. Driver-internal
  `connect_timeout` and `server_selection_timeout` are now set from
  `ctx.attempt_timeout` and the outer wrapper is removed, so the driver
  fails fast on its own terms instead of being yanked mid-work.
- RabbitMQ `conn.close` now uses AMQP 0-9-1 spec reply code 200
  (`REPLY_SUCCESS`) instead of 0.
- Temporal probe drops a silent `unwrap_or_else` fallback in
  `rewrite_url`; URL-rewrite failure now emits a real `err_stage` with
  hint instead of dispatching the wrong scheme to the gRPC probe.
- `tests/common/free_port` hardened against post-drop port-reuse races
  on busy CI runners by re-verifying the bound-then-dropped port is
  closed before returning.
- Parse-error message scrub now percent-decodes query keys before
  matching, so `?to%6Bken=...` cannot bypass the redaction.
- Two stray non-doc `//` comments removed per the project's zero-
  non-doc-comment rule.

## [0.1.2] - 2026-05-11

### Added

- `log:///path/to/file?match=needle` (substring) or
  `log:///path/to/file?regex=pattern` (compiled at parse time) target
  scheme. Reads the trailing 1 MiB of the file on each attempt, so it
  works against rotated multi-GB logs. UNC and remote hosts refused at
  parse time. Operator hints distinguish "pattern not yet present" from
  "log file missing".
- HTTP `--expect-body-regex <PATTERN>` to require the response body to
  match a regex. Uses `regex-lite` for a small binary footprint.
- HTTP `--expect-json <POINTER=VALUE>` to require an RFC 6901 JSON
  pointer (e.g. `/status=UP`, `/data/healthy=true`) to equal a literal
  value. Strings, booleans, and numbers match by their JSON form.
- Multiple body matchers compose: substring + regex + JSON pointer all
  must hold when set together.
- HTTP probe surfaces a sanitized body snippet (up to 240 chars) and the
  upstream `Server` / `X-Powered-By` / `Via` headers when the response
  status falls outside the expected range. Output now reads
  `status 503 [server: nginx/1.27]: {"error":"db not ready"}` instead of
  the bare `status 503`. Helps answer the "why is it 5xx?" question
  without an extra `curl -v`.
- gRPC `Health/Check` probe via `tonic` + rustls. URL forms `grpc://host:port`
  or `grpcs://host:port` for TLS, with optional `/Service` path to select a
  specific service. Behind the `grpc` cargo feature, bundled in `full`.
  Operator hints for `NOT_SERVING`, `UNIMPLEMENTED`, unknown service, TLS
  handshake failure, and auth.
- Shell completion script generation via hidden flag
  `--generate-completion <bash|zsh|fish|power-shell|elvish>` (writes to
  stdout). Man page generation via `--generate-manpage`.
- TOML configuration file via `--config <PATH>` (or env `HOLDON_CONFIG`).
  Auto-detects `holdon.toml` / `.holdon.toml` in the current directory when
  no flag is given. Supports global defaults (`interval`, `timeout`,
  `max_interval`, `initial_delay`, `attempt_timeout`, `success_threshold`,
  `jitter`, `sequential`, `reverse`, `once`, `at_least`) plus a `targets`
  array that is appended to CLI targets. Explicit CLI flags win over the
  config file.
- HTTP `--expect-body <SUBSTRING>` to require a literal substring in the
  response body. Body is capped at 1 MiB.
- HTTP `--no-follow-redirects` to disable redirect following.
- HTTP `--ca-cert <PATH>` to append PEM CA certificates to the bundled
  webpki roots.
- HTTP `--tls-min 1.2|1.3` to enforce a minimum TLS protocol version
  (defaults to 1.2).
- MySQL / MariaDB probe (`mysql://`, `mariadb://`) via `mysql_async` with
  rustls TLS by default. Opt out with `?ssl-mode=disable`. Behind the new
  `mysql` cargo feature. `all-databases` and `full` features include it.
- Mysql-specific operator hints for auth failure, missing database, TLS
  handshake failure, host-blocked, and not-ready states.

## [0.1.1] - 2026-05-11

### Fixed

- `file://` parser refuses raw `file:////...` prefixes on all platforms, not
  just Windows (UNC normalization in the `url` crate was hiding the case on
  Linux and macOS).
- Clippy `map_unwrap_or` lint on Rust 1.95+ in the terminal-width helper.
- CI: `cargo-deny` allows `CDLA-Permissive-2.0` (webpki-roots license) and
  ignores `RUSTSEC-2025-0134` (rustls-pemfile unmaintained).
- Release workflow: SBOM path glob matches per-crate cyclonedx output.
- Release workflow: Docker image build + push to `ghcr.io`.

### Yanked

- `0.1.0` was yanked from crates.io. Use `0.1.1` or later.

## [0.1.0] - 2026-05-10

### Added

- TCP, `host:port`, and `:port` shorthand
- HTTP / HTTPS probes with configurable status range, custom headers
  (`-H`), method (`--method`), and `--insecure` for self-signed certs
- DNS resolution probes (`dns://`)
- Postgres `SELECT 1` probes with rustls TLS (opportunistic upgrade,
  honors `?sslmode=disable`), behind `postgres` feature
- Redis `PING` probes with rustls TLS for `rediss://`, behind `redis`
  feature
- File existence and absence probes (`file://`)
- External command probes (`exec://`), the universal escape hatch
- Parallel orchestration with exponential backoff and jitter
- Diagnostic staged output (DNS, TCP, protocol stage)
- JSON output with stable schema (`v: 1`) and unix-ms timestamps
- Live matrix-dot spinner, sparklines, per-target latency
- Stdin target list (`-`) with comments and BOM stripping
- Cargo features: `http`, `postgres`, `redis`, `json-output`,
  `all-databases`, `full`
- POSIX exit codes: `0` ready, `2` misuse, `124` timeout, `126` exec
  permission, `127` exec not found, `130` / `143` signal

### Security

- TLS is rustls only, no OpenSSL anywhere in the tree
- `file://` probes use `symlink_metadata` (no symlink follow)
- HTTP redirects refuse `https → http` downgrades
- Passwords redacted in `Display`, `Debug`, and error chains
- `--insecure` emits a stderr warning on every run
- Stdin target ingest capped at 10 000 entries and 2 KiB per string

[Unreleased]: https://github.com/imjustprism/holdon/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/imjustprism/holdon/releases/tag/v0.2.1
[0.2.0]: https://github.com/imjustprism/holdon/releases/tag/v0.2.0
[0.1.2]: https://github.com/imjustprism/holdon/releases/tag/v0.1.2
[0.1.1]: https://github.com/imjustprism/holdon/releases/tag/v0.1.1
[0.1.0]: https://github.com/imjustprism/holdon/releases/tag/v0.1.0
