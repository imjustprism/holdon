use std::time::Instant;

use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Error as WsError;
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, install_rustls_provider_once, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::util::{format_error_chain, redact_in};

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    install_rustls_provider_once();
    let start = Instant::now();
    let pw = url.password().unwrap_or("").to_owned();
    let stage = match timeout(ctx.attempt_timeout, connect(url)).await {
        Ok(Ok(())) => ok_stage(StageKind::Ws, start.elapsed()),
        Ok(Err(e)) => {
            let hint = classify_error(&e);
            let mut msg = format_error_chain(&e);
            if !pw.is_empty() {
                msg = redact_in(&msg, &pw);
            }
            err_stage(StageKind::Ws, start.elapsed(), msg, Some(hint))
        }
        Err(_) => err_stage(StageKind::Ws, ctx.attempt_timeout, hints::TIMED_OUT, None),
    };
    vec![stage]
}

async fn connect(url: &Url) -> Result<(), WsError> {
    let (stream, _resp) = tokio_tungstenite::connect_async(url.as_str()).await?;
    drop(stream);
    Ok(())
}

const fn classify_error(e: &WsError) -> &'static str {
    match e {
        WsError::Io(_) => hints::WS_NO_CONNECT,
        WsError::Tls(_) => hints::WS_TLS,
        _ => hints::WS_HANDSHAKE,
    }
}
