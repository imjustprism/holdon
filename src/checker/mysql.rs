use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};
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

/// Rewrite `ssl-mode` to `REQUIRED` when TLS is requested but the URL leaves
/// it implicit or asks for `PREFERRED` (which lets the server downgrade to
/// plaintext and leak the SASL password).
fn enforce_sslmode(url: &Url, want_tls: bool) -> Url {
    if !want_tls {
        return url.clone();
    }
    let current = url
        .query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case("ssl-mode") || k.eq_ignore_ascii_case("sslmode"))
        .map(|(_, v)| v.into_owned().to_ascii_lowercase());
    let already_strict = current.as_deref().is_some_and(|v| {
        matches!(
            v,
            "required" | "verify_ca" | "verify-ca" | "verify_identity" | "verify-identity"
        )
    });
    if already_strict {
        return url.clone();
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("ssl-mode") && !k.eq_ignore_ascii_case("sslmode"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    let q = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .append_pair("ssl-mode", "REQUIRED")
        .finish();
    out.set_query(Some(&q));
    out
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn enforce_passthrough_when_disabled() {
        let url = Url::parse("mysql://u@h/?ssl-mode=DISABLED").unwrap();
        assert_eq!(enforce_sslmode(&url, false).as_str(), url.as_str());
    }

    #[test]
    fn enforce_appends_required_when_missing() {
        let url = Url::parse("mysql://u@h/").unwrap();
        let out = enforce_sslmode(&url, true);
        assert!(out.query_pairs().any(|(k, v)| k == "ssl-mode" && v == "REQUIRED"));
    }

    #[test]
    fn enforce_upgrades_preferred_to_required() {
        let url = Url::parse("mysql://u@h/?ssl-mode=PREFERRED").unwrap();
        let out = enforce_sslmode(&url, true);
        assert!(out.query_pairs().any(|(k, v)| k == "ssl-mode" && v == "REQUIRED"));
    }

    #[test]
    fn enforce_keeps_verify_identity() {
        let url = Url::parse("mysql://u@h/?ssl-mode=VERIFY_IDENTITY").unwrap();
        let out = enforce_sslmode(&url, true);
        assert!(out.query_pairs().any(|(k, v)| k == "ssl-mode" && v == "VERIFY_IDENTITY"));
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
