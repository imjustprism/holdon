# Changelog

All notable changes to this project will be documented in this file. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). This
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

[Unreleased]: https://github.com/imjustprism/holdon/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/imjustprism/holdon/releases/tag/v0.1.1
[0.1.0]: https://github.com/imjustprism/holdon/releases/tag/v0.1.0
