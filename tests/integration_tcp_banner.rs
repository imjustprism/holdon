#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::time::Duration;

mod common;
use common::{bind_ephemeral, quick_cfg, run};
use holdon::Target;
use tokio::io::AsyncWriteExt;

async fn spawn_banner_server(banner: Option<Vec<u8>>) -> u16 {
    let (listener, port) = bind_ephemeral().await;
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let line = banner.clone();
            tokio::spawn(async move {
                if let Some(b) = line {
                    let _ = sock.write_all(&b).await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_banner_substring_matches() {
    let port = spawn_banner_server(Some(b"220 smtp.example.org ESMTP\r\n".to_vec())).await;
    let target: Target = format!("tcp://127.0.0.1:{port}?expect-banner=220")
        .parse()
        .unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_banner_mismatch_fails() {
    let port = spawn_banner_server(Some(b"hello\r\n".to_vec())).await;
    let target: Target = format!("tcp://127.0.0.1:{port}?expect-banner=ESMTP")
        .parse()
        .unwrap();
    let cfg = quick_cfg(800).interval(Duration::from_millis(50));
    let report = run(cfg, vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_banner_regex_matches() {
    let port = spawn_banner_server(Some(b"SSH-2.0-OpenSSH_9.3\r\n".to_vec())).await;
    let target: Target = format!("tcp://127.0.0.1:{port}?expect-banner-regex=%5ESSH-2%5C.%5Cd")
        .parse()
        .unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_banner_silent_server_times_out() {
    let port = spawn_banner_server(None).await;
    let target: Target = format!("tcp://127.0.0.1:{port}?expect-banner=220")
        .parse()
        .unwrap();
    let cfg = quick_cfg(800).interval(Duration::from_millis(50));
    let report = run(cfg, vec![target]).await;
    assert!(!report.all_ready());
}
