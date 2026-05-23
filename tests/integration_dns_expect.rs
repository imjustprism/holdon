#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::time::Duration;

mod common;
use common::{quick_cfg, run};
use holdon::Target;

#[tokio::test]
async fn dns_expect_ip_matches_localhost() {
    let target: Target = "dns://localhost?expect-ip=127.0.0.1".parse().unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test]
async fn dns_expect_ip_mismatch_fails() {
    let target: Target = "dns://localhost?expect-ip=10.255.255.254".parse().unwrap();
    let cfg = quick_cfg(800).interval(Duration::from_millis(50));
    let report = run(cfg, vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test]
async fn dns_no_expect_resolves() {
    let target: Target = "dns://localhost".parse().unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}
