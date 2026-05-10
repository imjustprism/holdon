#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::time::Duration;

mod common;
use common::{bind_ephemeral, free_port, quick_cfg, run, tcp_target};

#[tokio::test]
async fn tcp_ready_when_listener_bound() {
    let (_listener, port) = bind_ephemeral().await;
    let cfg = quick_cfg(5000);
    let report = run(cfg, vec![tcp_target(port)]).await;
    assert!(report.all_ready());
}

#[tokio::test]
async fn tcp_fails_on_closed_port() {
    let port = free_port().await;
    let cfg = quick_cfg(500)
        .interval(Duration::from_millis(50))
        .max_interval(Duration::from_millis(100));
    let report = run(cfg, vec![tcp_target(port)]).await;
    assert!(!report.all_ready());
}

#[tokio::test]
async fn reverse_succeeds_when_port_closed() {
    let port = free_port().await;
    let cfg = quick_cfg(2000).reverse(true);
    let report = run(cfg, vec![tcp_target(port)]).await;
    assert!(report.all_ready());
}
