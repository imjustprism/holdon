use std::time::Instant;

use futures_util::StreamExt;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, install_rustls_provider_once, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::LogMatcher;
use crate::util::{format_error_chain, redact_in};

pub(super) async fn probe(url: &Url, expect: Option<&LogMatcher>, ctx: AttemptCtx) -> Vec<Stage> {
    install_rustls_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let stage = match timeout(ctx.attempt_timeout, run(url, expect)).await {
        Ok(Ok(())) => ok_stage(StageKind::Ws, start.elapsed()),
        Ok(Err(e)) => {
            let mut msg = e.message;
            if !pw.is_empty() {
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Ws, start.elapsed(), msg, Some(e.hint))
        }
        Err(_) => err_stage(StageKind::Ws, ctx.attempt_timeout, hints::TIMED_OUT, None),
    };
    vec![stage]
}

struct ProbeErr {
    message: String,
    hint: &'static str,
}

async fn run(url: &Url, expect: Option<&LogMatcher>) -> Result<(), ProbeErr> {
    let (mut stream, _resp) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .map_err(|e| ProbeErr {
            message: format_error_chain(&e),
            hint: classify_error(&e),
        })?;
    let Some(matcher) = expect else {
        drop(stream);
        return Ok(());
    };
    let text = loop {
        let frame = stream.next().await.ok_or_else(|| ProbeErr {
            message: "connection closed before a data frame was received".to_owned(),
            hint: hints::WS_NO_MESSAGE,
        })?;
        let msg = frame.map_err(|e| ProbeErr {
            message: format_error_chain(&e),
            hint: classify_error(&e),
        })?;
        match msg {
            Message::Text(t) => break t.to_string(),
            Message::Binary(b) => match std::str::from_utf8(&b) {
                Ok(s) => break s.to_owned(),
                Err(_) => {
                    return Err(ProbeErr {
                        message: "first data frame was non-UTF-8 binary".to_owned(),
                        hint: hints::WS_BINARY_MESSAGE,
                    });
                }
            },
            Message::Close(_) => {
                return Err(ProbeErr {
                    message: "server sent Close before any data frame".to_owned(),
                    hint: hints::WS_NO_MESSAGE,
                });
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    };
    let hit = match matcher {
        LogMatcher::Substring(s) => text.contains(s.as_str()),
        LogMatcher::Regex(re) => re.is_match(&text),
    };
    if hit {
        Ok(())
    } else {
        Err(ProbeErr {
            message: "received message did not match the expected pattern".to_owned(),
            hint: hints::WS_MESSAGE_MISMATCH,
        })
    }
}

const fn classify_error(e: &WsError) -> &'static str {
    match e {
        WsError::Io(_) => hints::WS_NO_CONNECT,
        WsError::Tls(_) => hints::WS_TLS,
        _ => hints::WS_HANDSHAKE,
    }
}
