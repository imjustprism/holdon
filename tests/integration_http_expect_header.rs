#![cfg(feature = "http")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod common;

use std::sync::Once;
use std::time::Duration;

use common::{quick_cfg, run};
use holdon::Target;
use holdon::checker::http::{HeaderName, HttpConfig, set_global};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static INIT: Once = Once::new();

fn install_expectation() {
    INIT.call_once(|| {
        let mut cfg = HttpConfig::defaults();
        cfg.header_expectations.push((
            HeaderName::from_static("x-status"),
            regex_lite::Regex::new("^ok$").unwrap(),
        ));
        set_global(cfg);
    });
}

async fn spawn_http_server(extra_header: Option<&'static str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = tokio::time::timeout(Duration::from_millis(500), sock.read(&mut buf)).await;
            let header_line = extra_header.map(|h| format!("{h}\r\n")).unwrap_or_default();
            let body = "ok";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{header_line}Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn header_match_is_ready() {
    install_expectation();
    let port = spawn_http_server(Some("X-Status: ok")).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(2000), vec![target]).await;
    assert!(report.all_ready(), "expected ready, got {report:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn header_value_mismatch_is_not_ready() {
    install_expectation();
    let port = spawn_http_server(Some("X-Status: bad")).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn header_missing_is_not_ready() {
    install_expectation();
    let port = spawn_http_server(None).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    assert!(!report.all_ready());
}

async fn spawn_non_ascii_header_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = tokio::time::timeout(Duration::from_millis(500), sock.read(&mut buf)).await;
            let mut resp: Vec<u8> = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Status: ".to_vec();
            resp.extend_from_slice(&[0xC0, 0xC1, 0xFF]);
            resp.extend_from_slice(b"\r\nConnection: close\r\n\r\nok");
            let _ = sock.write_all(&resp).await;
            let _ = sock.shutdown().await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn header_non_ascii_value_uses_encoding_hint() {
    install_expectation();
    let port = spawn_non_ascii_header_server().await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick_cfg(800), vec![target]).await;
    assert!(!report.all_ready());
    let stage = report.results[0]
        .final_outcome
        .stages
        .last()
        .expect("at least one stage");
    let holdon::diagnostic::StageResult::Err { hint, message } = &stage.result else {
        panic!("expected Err stage, got Ok");
    };
    let hint_text = hint.as_deref().unwrap_or_default();
    assert!(
        hint_text.contains("non-ASCII"),
        "expected encoding hint, got `{hint_text}` (message: {message})"
    );
}
