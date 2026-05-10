#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod common;
use common::{absent_file_target, file_target, quick_cfg, run};

#[tokio::test]
async fn file_present_passes() {
    let (_dir, target) = file_target("ready", Some(b"x"));
    let report = run(quick_cfg(2000), vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test]
async fn file_absent_mode_passes_when_missing() {
    let (_dir, target) = absent_file_target("never");
    let report = run(quick_cfg(2000), vec![target]).await;
    assert!(report.all_ready());
}
