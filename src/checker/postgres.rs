use std::sync::Arc;
use std::sync::OnceLock;

use rustls::ClientConfig;
use tokio_postgres::NoTls;
use tokio_postgres_rustls::MakeRustlsConnect;
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
            StageKind::Postgres,
            ctx.attempt_timeout,
            hints::PG_NOT_READY,
            connect_and_query(conn_str, want_tls),
            &[conn_str, pw],
        )
        .await,
    ]
}

fn sslmode_disabled(url: &Url) -> bool {
    url.query_pairs()
        .any(|(k, v)| k.eq_ignore_ascii_case("sslmode") && v.eq_ignore_ascii_case("disable"))
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

async fn connect_and_query(conn_str: &str, want_tls: bool) -> Result<(), tokio_postgres::Error> {
    if want_tls {
        let tls = MakeRustlsConnect::new(rustls_config().as_ref().clone());
        let (client, connection) = tokio_postgres::connect(conn_str, tls).await?;
        let driver = tokio::spawn(connection);
        let query = client.simple_query("SELECT 1").await.map(|_| ());
        drop(client);
        match driver.await {
            Ok(Err(e)) => query.and(Err(e)),
            Ok(Ok(())) | Err(_) => query,
        }
    } else {
        let (client, connection) = tokio_postgres::connect(conn_str, NoTls).await?;
        let driver = tokio::spawn(connection);
        let query = client.simple_query("SELECT 1").await.map(|_| ());
        drop(client);
        match driver.await {
            Ok(Err(e)) => query.and(Err(e)),
            Ok(Ok(())) | Err(_) => query,
        }
    }
}
