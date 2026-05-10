#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod common;

use assert_cmd::Command;
use common::{bind_ephemeral, free_port};
use predicates::prelude::*;

fn cmd() -> Command {
    let mut c = Command::cargo_bin("holdon").unwrap();
    c.env("HOLDON_ASCII", "1").env("NO_COLOR", "1");
    c
}

#[test]
fn shows_version() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("holdon"));
}

#[test]
fn shows_help() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Wait for anything"))
        .stdout(predicate::str::contains("--timeout"))
        .stdout(predicate::str::contains("--strict"));
}

#[test]
fn no_args_fails_with_misuse() {
    cmd().assert().code(2);
}

#[test]
fn invalid_target_fails_with_misuse() {
    cmd()
        .arg("not-a-target")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("holdon:"));
}

#[test]
fn unc_path_refused_for_security() {
    cmd()
        .arg("file://attacker.com/share/x")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("UNC").or(predicate::str::contains("NTLM")));
}

#[test]
fn ambiguous_ipv6_rejected() {
    cmd()
        .arg("::1:5432")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("IPv6"));
}

#[test]
fn duration_nan_rejected() {
    cmd().arg(":1").arg("--timeout").arg("NaN").assert().code(2);
}

#[test]
fn duration_infinity_rejected() {
    cmd().arg(":1").arg("--timeout").arg("inf").assert().code(2);
}

#[test]
fn duration_negative_rejected() {
    cmd().arg(":1").arg("--timeout").arg("-5s").assert().code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_against_listening_port() {
    let (_listener, port) = bind_ephemeral().await;
    let mut c = Command::cargo_bin("holdon").unwrap();
    c.env("HOLDON_ASCII", "1")
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--timeout")
        .arg("3s")
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(0);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeout_against_closed_port() {
    let port = free_port().await;
    let mut c = Command::cargo_bin("holdon").unwrap();
    c.env("HOLDON_ASCII", "1")
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--timeout")
        .arg("500ms")
        .arg("--attempt-timeout")
        .arg("200ms")
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(124);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeout_exit_code_override() {
    let port = free_port().await;
    Command::cargo_bin("holdon")
        .unwrap()
        .env("HOLDON_ASCII", "1")
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--timeout")
        .arg("500ms")
        .arg("--attempt-timeout")
        .arg("200ms")
        .arg("--timeout-exit-code")
        .arg("1")
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(1);
}

#[cfg(feature = "json-output")]
#[tokio::test(flavor = "multi_thread")]
async fn json_output_emits_valid_ndjson() {
    let port = free_port().await;
    let out = Command::cargo_bin("holdon")
        .unwrap()
        .env("HOLDON_ASCII", "1")
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--output")
        .arg("json")
        .arg("--timeout")
        .arg("500ms")
        .arg("--attempt-timeout")
        .arg("200ms")
        .arg(format!("127.0.0.1:{port}"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"event\":\"start\""), "stdout={stdout}");
    assert!(stdout.contains("\"event\":\"end\""), "stdout={stdout}");
    assert!(stdout.contains("\"v\":1"), "stdout={stdout}");
    for line in stdout.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|_| panic!("invalid json line: {line}"));
        assert!(v.get("v").is_some(), "missing v: {line}");
        assert!(v.get("event").is_some(), "missing event: {line}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn quiet_suppresses_stderr() {
    let (_listener, port) = bind_ephemeral().await;
    let out = Command::cargo_bin("holdon")
        .unwrap()
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--quiet")
        .arg("--timeout")
        .arg("3s")
        .arg(format!("127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stderr.is_empty(),
        "stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reverse_succeeds_when_port_closed() {
    let port = free_port().await;
    Command::cargo_bin("holdon")
        .unwrap()
        .env("HOLDON_ASCII", "1")
        .env("NO_COLOR", "1")
        .arg("--once")
        .arg("--reverse")
        .arg("--timeout")
        .arg("2s")
        .arg(format!("127.0.0.1:{port}"))
        .assert()
        .code(0);
}

#[test]
fn short_flags_t_and_s_recognized() {
    cmd()
        .arg("--help")
        .assert()
        .stdout(predicate::str::contains("-t"))
        .stdout(predicate::str::contains("-s"));
}

#[cfg(feature = "http")]
#[test]
fn rejects_malformed_header() {
    cmd()
        .arg("-H")
        .arg("not-a-header")
        .arg("--once")
        .arg("--timeout")
        .arg("1s")
        .arg("http://127.0.0.1:1")
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing `:`"));
}

#[cfg(feature = "http")]
#[test]
fn http_method_help_lists_options() {
    cmd()
        .arg("--help")
        .assert()
        .stdout(predicate::str::contains("--method"))
        .stdout(predicate::str::contains("--insecure"))
        .stdout(predicate::str::contains("-H, --header"));
}
