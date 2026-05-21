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

use common::run;
use holdon::Target;
use holdon::checker::http::HttpConfig;
use holdon::runner::RunnerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_slow_http_server(delay: Duration) -> u16 {
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
            tokio::time::sleep(delay).await;
            let body = "ok";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn http_response_slower_than_max_rtt_is_not_ready() {
    let port = spawn_slow_http_server(Duration::from_millis(400)).await;
    holdon::checker::http::set_global(HttpConfig {
        max_rtt: Some(Duration::from_millis(50)),
        follow_redirects: true,
        ..HttpConfig::default()
    });
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(1500))
        .attempt_timeout(Duration::from_millis(1500))
        .interval(Duration::from_millis(100))
        .max_interval(Duration::from_millis(100));
    let report = run(cfg, vec![target]).await;
    assert!(
        !report.all_ready(),
        "max-rtt should reject a server slower than the limit"
    );
    let r = report.results.first().expect("one target");
    assert!(!r.satisfied);
    let stage = r.final_outcome.stages.last().expect("at least one stage");
    let msg = match &stage.result {
        holdon::diagnostic::StageResult::Err { message, .. } => message.to_string(),
        _ => panic!("expected err stage"),
    };
    assert!(
        msg.contains("max-rtt"),
        "expected max-rtt message, got: {msg}"
    );
}
