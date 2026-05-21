#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::literal_string_with_formatting_args
)]

use std::process::Command;

fn cmd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_holdon"));
    c.env("HOLDON_ASCII", "1").env("NO_COLOR", "1");
    c
}

#[test]
fn env_placeholder_resolves_but_secret_value_never_logged() {
    let out = cmd()
        .env("HOLDON_SECRET_TEST", "expanded-value-marker")
        .arg("--timeout")
        .arg("100ms")
        .arg("--attempt-timeout")
        .arg("100ms")
        .arg("--once")
        .arg("not-a-target://${env:HOLDON_SECRET_TEST}")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("expanded-value-marker"),
        "secret leaked into stderr: {stderr}"
    );
    assert!(
        stderr.contains("${env:HOLDON_SECRET_TEST}"),
        "expected original placeholder in stderr: {stderr}"
    );
}

#[test]
fn missing_env_var_fails_at_startup() {
    let out = cmd()
        .arg("--once")
        .arg("tcp://${env:HOLDON_TEST_NEVER_DEFINED_XYZ123}:1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HOLDON_TEST_NEVER_DEFINED_XYZ123"),
        "stderr={stderr}"
    );
    assert_eq!(out.status.code(), Some(2));
}
