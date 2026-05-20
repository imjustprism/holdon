#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
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
fn env_placeholder_resolved_in_target_string() {
    // We do not actually need the target to be reachable. We use the
    // parse error path: holdon prints the resolved target string when
    // parsing fails, so we can verify substitution happened by looking
    // for the expanded value in stderr.
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
        stderr.contains("expanded-value-marker"),
        "stderr={stderr}"
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
