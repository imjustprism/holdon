use std::sync::Arc;
use std::sync::OnceLock;

use rustls::ClientConfig;
use tokio_postgres::NoTls;
use tokio_postgres_rustls::MakeRustlsConnect;
use url::Url;

use super::hint::{Hintable, hints};
use super::{AttemptCtx, run_stage};
use crate::diagnostic::{Stage, StageKind};

pub(super) async fn probe(url: &Url, expect_table: Option<&str>, ctx: AttemptCtx) -> Vec<Stage> {
    let pw = url.password().unwrap_or("").to_owned();
    let want_tls = !sslmode_disabled(url);
    let driver_url = strip_table_param(url);
    let driver_str = driver_url.as_str().to_owned();
    let table = expect_table.map(str::to_owned);
    vec![
        run_stage(
            StageKind::Postgres,
            ctx.attempt_timeout,
            hints::PG_NOT_READY,
            connect_and_query(driver_str.clone(), want_tls, table),
            &[driver_str.as_str(), pw.as_str()],
        )
        .await,
    ]
}

fn sslmode_disabled(url: &Url) -> bool {
    url.query_pairs()
        .any(|(k, v)| k.eq_ignore_ascii_case("sslmode") && v.eq_ignore_ascii_case("disable"))
}

fn strip_table_param(url: &Url) -> Url {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k.as_ref() != "table")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    if kept.is_empty() {
        out.set_query(None);
    } else {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish();
        out.set_query(Some(&q));
    }
    out
}

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error(transparent)]
    Driver(#[from] tokio_postgres::Error),
    #[error("expected table `{0}` not found in information_schema.tables")]
    TableMissing(String),
}

impl Hintable for ProbeError {
    fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Driver(e) => e.hint(),
            Self::TableMissing(_) => Some(hints::PG_TABLE_MISSING),
        }
    }
}

fn rustls_config() -> Arc<ClientConfig> {
    static CFG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    })
    .clone()
}

async fn connect_and_query(
    conn_str: String,
    want_tls: bool,
    expect_table: Option<String>,
) -> Result<(), ProbeError> {
    if want_tls {
        let tls = MakeRustlsConnect::new(rustls_config().as_ref().clone());
        let (client, connection) = tokio_postgres::connect(&conn_str, tls).await?;
        let driver = tokio::spawn(connection);
        let outcome = run_queries(&client, expect_table.as_deref()).await;
        drop(client);
        let _ = driver.await;
        outcome
    } else {
        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls).await?;
        let driver = tokio::spawn(connection);
        let outcome = run_queries(&client, expect_table.as_deref()).await;
        drop(client);
        let _ = driver.await;
        outcome
    }
}

async fn run_queries(
    client: &tokio_postgres::Client,
    expect_table: Option<&str>,
) -> Result<(), ProbeError> {
    client.simple_query("SELECT 1").await?;
    if let Some(name) = expect_table {
        let row = client
            .query_opt(
                "SELECT 1 FROM information_schema.tables \
                 WHERE table_name = $1 AND table_schema = ANY (current_schemas(false)) \
                 LIMIT 1",
                &[&name],
            )
            .await?;
        if row.is_none() {
            return Err(ProbeError::TableMissing(name.to_owned()));
        }
    }
    Ok(())
}
