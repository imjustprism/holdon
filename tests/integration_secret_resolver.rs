#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::literal_string_with_formatting_args
)]

//! End-to-end coverage for the env-source secret resolver. Lives here
//! because `std::env::set_var` is unsafe under Rust 2024 and the main
//! binary forbids unsafe; integration tests have their own crate root
//! and can opt in to unsafe.

use std::process::Command;

fn cmd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_holdon"));
    c.env("HOLDON_ASCII", "1").env("NO_COLOR", "1");
    c
}

#[test]
fn env_placeholder_resolves_but_secret_value_never_logged() {
    // Two guarantees:
    //  1. Resolution happens before URL parsing (the env var is read).
    //  2. The substituted value never appears in stderr, only the
    //     original placeholder form. This is the contract that lets
    //     operators safely use `${env:SECRET}` in CI without log
    //     collectors capturing the secret.
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
