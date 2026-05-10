#![allow(dead_code, unreachable_pub, clippy::unwrap_used)]

use std::time::Duration;

use holdon::runner::RunnerConfig;
use holdon::{Report, Runner, Target};
use tempfile::TempDir;
use tokio::net::TcpListener;

pub fn quick_cfg(timeout_ms: u64) -> RunnerConfig {
    RunnerConfig::default()
        .timeout(Duration::from_millis(timeout_ms))
        .attempt_timeout(Duration::from_millis(timeout_ms.min(1000)))
}

pub async fn run(cfg: RunnerConfig, targets: Vec<Target>) -> Report {
    Runner::new(cfg).run(targets, None).await
}

pub async fn bind_ephemeral() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

pub async fn free_port() -> u16 {
    let (l, p) = bind_ephemeral().await;
    drop(l);
    p
}

pub fn tcp_target(port: u16) -> Target {
    format!("127.0.0.1:{port}").parse().unwrap()
}

pub fn file_target(name: &str, contents: Option<&[u8]>) -> (TempDir, Target) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    if let Some(b) = contents {
        std::fs::write(&path, b).unwrap();
    }
    let url = url::Url::from_file_path(&path).unwrap();
    let t: Target = url.as_str().parse().unwrap();
    (dir, t)
}

pub fn absent_file_target(name: &str) -> (TempDir, Target) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    let url = format!("{}?mode=absent", url::Url::from_file_path(&path).unwrap());
    let t: Target = url.parse().unwrap();
    (dir, t)
}
