#![cfg(feature = "http")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::manual_let_else
)]

mod common;

use std::time::Duration;

use common::{quick_cfg, run};
use holdon::Target;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_http_server(status: u16, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let mut buf = [0u8; 4096];
            let _ = tokio::time::timeout(Duration::from_millis(500), sock.read(&mut buf)).await;
            let resp = format!(
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn http_200_is_ready() {
    let port = spawn_http_server(200, "ok").await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(2000), vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_404_is_not_ready() {
    let port = spawn_http_server(404, "nope").await;
    let target: Target = format!("http://127.0.0.1:{port}/x").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_503_is_not_ready_during_drain() {
    let port = spawn_http_server(503, "draining").await;
    let target: Target = format!("http://127.0.0.1:{port}/health").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_204_no_content_is_ready_2xx_default() {
    let port = spawn_http_server(204, "").await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(2000), vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_redirect_302_not_ready_under_strict_2xx() {
    // Default StatusRange is 200..=299. A 302 response that we DO NOT follow
    // (because the Location target doesn't exist) must NOT count as ready.
    let port = spawn_http_server(302, "moved").await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    // reqwest follows redirects up to 5; the dummy server returns 302 with no
    // Location, which reqwest then surfaces as a redirect error → not ready.
    assert!(!report.all_ready());
}
