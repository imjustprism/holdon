use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};
use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, run_stage};
use crate::diagnostic::{Stage, StageKind};

pub(super) async fn probe(url: &Url, expect_table: Option<&str>, ctx: AttemptCtx) -> Vec<Stage> {
    let pw = url.password().unwrap_or("").to_owned();
    let want_tls = !sslmode_disabled(url);
    let driver_url = super::strip_query_keys(url, &["table"]);
    let driver_str = driver_url.as_str().to_owned();
    let table = expect_table.map(str::to_owned);
    vec![
        run_stage(
            StageKind::Mysql,
            ctx.attempt_timeout,
            hints::MYSQL_NOT_READY,
            connect_and_query(driver_str.clone(), want_tls, table),
            &[driver_str.as_str(), pw.as_str()],
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

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error(transparent)]
    Driver(#[from] mysql_async::Error),
    #[error("expected table `{0}` not found in information_schema.tables")]
    TableMissing(String),
}

impl Hintable for ProbeError {
    fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Driver(e) => e.hint(),
            Self::TableMissing(_) => Some(hints::MYSQL_TABLE_MISSING),
        }
    }
}

async fn connect_and_query(
    conn_str: String,
    want_tls: bool,
    expect_table: Option<String>,
) -> Result<(), ProbeError> {
    crate::checker::install_rustls_provider_once();
    let normalized: String;
    let for_opts: &str = if let Some(rest) = conn_str.strip_prefix("mariadb://") {
        normalized = format!("mysql://{rest}");
        normalized.as_str()
    } else {
        conn_str.as_str()
    };
    let base = Opts::from_url(for_opts).map_err(mysql_async::Error::Url)?;
    let mut builder = OptsBuilder::from_opts(base);
    if want_tls {
        builder = builder.ssl_opts(Some(SslOpts::default()));
    } else {
        builder = builder.ssl_opts(None);
    }
    let mut conn = Conn::new(builder).await?;
    let _: Vec<u8> = conn.query("SELECT 1").await?;
    if let Some(name) = expect_table {
        let rows: Vec<u8> = conn
            .exec(
                "SELECT 1 FROM information_schema.tables \
                 WHERE table_name = ? AND table_schema = DATABASE() \
                 LIMIT 1",
                (name.as_str(),),
            )
            .await?;
        if rows.is_empty() {
            let _ = conn.disconnect().await;
            return Err(ProbeError::TableMissing(name));
        }
    }
    conn.disconnect().await?;
    Ok(())
}
