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
    let driver_url = enforce_sslmode(&super::strip_query_keys(url, &["table"]), want_tls);
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

/// Force `sslmode=require` when TLS is requested but the URL leaves the floor
/// implicit (`prefer`/`allow`/missing). Without this, libpq semantics let a
/// MITM strip TLS by responding `N` to the SSL request, then the driver
/// silently sends the password in cleartext on the same socket.
///
/// Explicit `require`/`verify-ca`/`verify-full` are passed through untouched.
fn enforce_sslmode(url: &Url, want_tls: bool) -> Url {
    if !want_tls {
        return url.clone();
    }
    let current = url
        .query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case("sslmode"))
        .map(|(_, v)| v.into_owned().to_ascii_lowercase());
    let already_strict = current
        .as_deref()
        .is_some_and(|v| matches!(v, "require" | "verify-ca" | "verify-full"));
    if already_strict {
        return url.clone();
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("sslmode"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    let q = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .append_pair("sslmode", "require")
        .finish();
    out.set_query(Some(&q));
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn enforce_passthrough_when_disabled() {
        let url = Url::parse("postgres://u@h/?sslmode=disable").unwrap();
        assert_eq!(enforce_sslmode(&url, false).as_str(), url.as_str());
    }

    #[test]
    fn enforce_appends_require_when_missing() {
        let url = Url::parse("postgres://u@h/").unwrap();
        let out = enforce_sslmode(&url, true);
        assert!(
            out.query_pairs()
                .any(|(k, v)| k == "sslmode" && v == "require")
        );
    }

    #[test]
    fn enforce_upgrades_prefer_to_require() {
        let url = Url::parse("postgres://u@h/?sslmode=prefer&x=1").unwrap();
        let out = enforce_sslmode(&url, true);
        let pairs: Vec<_> = out.query_pairs().collect();
        assert!(pairs.iter().any(|(k, v)| k == "sslmode" && v == "require"));
        assert!(pairs.iter().any(|(k, v)| k == "x" && v == "1"));
        assert!(pairs.iter().filter(|(k, _)| k == "sslmode").count() == 1);
    }

    #[test]
    fn enforce_keeps_verify_full() {
        let url = Url::parse("postgres://u@h/?sslmode=verify-full").unwrap();
        let out = enforce_sslmode(&url, true);
        assert!(
            out.query_pairs()
                .any(|(k, v)| k == "sslmode" && v == "verify-full")
        );
    }
}
