use std::sync::OnceLock;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, run_stage};
use crate::diagnostic::{Stage, StageKind};

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    let conn_str = url.as_str();
    let pw = url.password().unwrap_or("");
    let want_tls = !sslmode_disabled(url);
    vec![
        run_stage(
            StageKind::Mysql,
            ctx.attempt_timeout,
            hints::MYSQL_NOT_READY,
            connect_and_query(conn_str, want_tls),
            &[conn_str, pw],
        )
        .await,
    ]
}

fn sslmode_disabled(url: &Url) -> bool {
    url.query_pairs().any(|(k, v)| {
        (k.eq_ignore_ascii_case("ssl-mode") || k.eq_ignore_ascii_case("sslmode"))
            && (v.eq_ignore_ascii_case("disable")
                || v.eq_ignore_ascii_case("disabled")
                || v.eq_ignore_ascii_case("off"))
    })
}

fn install_provider_once() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn connect_and_query(conn_str: &str, want_tls: bool) -> mysql_async::Result<()> {
    install_provider_once();
    let base = Opts::from_url(conn_str)?;
    let mut builder = OptsBuilder::from_opts(base);
    if want_tls {
        builder = builder.ssl_opts(Some(SslOpts::default()));
    } else {
        builder = builder.ssl_opts(None);
    }
    let mut conn = Conn::new(builder).await?;
    let _: Vec<u8> = conn.query("SELECT 1").await?;
    conn.disconnect().await?;
    Ok(())
}
