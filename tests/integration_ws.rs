#![cfg(feature = "websocket")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::time::Duration;

mod common;
use common::{bind_ephemeral, quick_cfg, run};
use futures_util::{SinkExt, StreamExt};
use holdon::Target;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

async fn spawn_echo_server(send: Option<String>) -> u16 {
    let (listener, port) = bind_ephemeral().await;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let send_clone = send.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = accept_async(stream).await else {
                    return;
                };
                if let Some(s) = send_clone {
                    let _ = ws.send(Message::Text(s.into())).await;
                }
                while let Some(Ok(_)) = ws.next().await {}
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_connect_only_succeeds() {
    let port = spawn_echo_server(None).await;
    let target: Target = format!("ws://127.0.0.1:{port}/").parse().unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_expect_text_matches_substring() {
    let port = spawn_echo_server(Some("hello world".to_owned())).await;
    let target: Target = format!("ws://127.0.0.1:{port}/?expect-text=world")
        .parse()
        .unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_expect_text_fails_on_mismatch() {
    let port = spawn_echo_server(Some("not the message".to_owned())).await;
    let target: Target = format!("ws://127.0.0.1:{port}/?expect-text=missing")
        .parse()
        .unwrap();
    let cfg = quick_cfg(800).interval(Duration::from_millis(50));
    let report = run(cfg, vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_expect_regex_matches() {
    let port = spawn_echo_server(Some(r#"{"status":"ok","build":"42"}"#.to_owned())).await;
    let target: Target = format!("ws://127.0.0.1:{port}/?expect-regex=%22status%22%3A%22ok%22")
        .parse()
        .unwrap();
    let cfg = quick_cfg(3000);
    let report = run(cfg, vec![target]).await;
    assert!(report.all_ready());
}
