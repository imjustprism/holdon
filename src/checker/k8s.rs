use std::time::Instant;

use reqwest::Certificate;
use tokio::time::timeout;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};
use crate::target::K8sKind;
use crate::util::sanitize_for_terminal;

const IN_POD_TOKEN: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const IN_POD_CA: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error("no Kubernetes config: {0}")]
    NoConfig(String),
    #[error("API server unreachable: {0}")]
    Connect(String),
    #[error("API server rejected bearer token (HTTP 401/403)")]
    Auth,
    #[error("resource not found")]
    NotFound,
    #[error("resource not yet ready: {0}")]
    NotReady(String),
    #[error("job has permanently failed: {0}")]
    JobFailed(String),
    #[error("unexpected API response: {0}")]
    Protocol(String),
}

impl ProbeError {
    const fn hint(&self) -> &'static str {
        match self {
            Self::NoConfig(_) => hints::K8S_NO_CONFIG,
            Self::Connect(_) => hints::K8S_API_UNREACHABLE,
            Self::Auth => hints::K8S_AUTH,
            Self::NotFound => hints::K8S_NOT_FOUND,
            Self::NotReady(_) => hints::K8S_NOT_READY,
            Self::JobFailed(_) => hints::K8S_JOB_FAILED,
            Self::Protocol(_) => hints::K8S_PROTOCOL,
        }
    }
}

pub(super) async fn probe(
    kind: K8sKind,
    namespace: &str,
    name: &str,
    conditions: &[String],
    ctx: AttemptCtx,
) -> Vec<Stage> {
    let start = Instant::now();
    let stage = match timeout(ctx.attempt_timeout, run(kind, namespace, name, conditions)).await {
        Ok(Ok(())) => ok_stage(StageKind::K8s, start.elapsed()),
        Ok(Err(e)) => {
            let h = e.hint();
            err_stage(
                StageKind::K8s,
                start.elapsed(),
                sanitize_for_terminal(&e.to_string()),
                Some(h),
            )
        }
        Err(_) => err_stage(
            StageKind::K8s,
            ctx.attempt_timeout,
            hints::TIMED_OUT,
            Some(hints::K8S_API_UNREACHABLE),
        ),
    };
    vec![stage]
}

async fn run(
    kind: K8sKind,
    namespace: &str,
    name: &str,
    conditions: &[String],
) -> Result<(), ProbeError> {
    let cfg = load_config()?;
    let url = format!(
        "{}{}",
        cfg.server.trim_end_matches('/'),
        resource_path(kind, namespace, name)
    );
    let resp = cfg
        .client
        .get(&url)
        .bearer_auth(&cfg.token)
        .send()
        .await
        .map_err(|e| ProbeError::Connect(format!("{e}")))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ProbeError::Auth);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ProbeError::NotFound);
    }
    if !status.is_success() {
        return Err(ProbeError::Protocol(format!("HTTP {status} from {url}")));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| ProbeError::Connect(format!("reading response body: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| ProbeError::Protocol(format!("API response was not JSON: {e}")))?;
    if conditions.is_empty() {
        check_ready(kind, &value)
    } else {
        check_explicit_conditions(kind, &value, conditions)
    }
}

fn check_explicit_conditions(
    kind: K8sKind,
    v: &serde_json::Value,
    conditions: &[String],
) -> Result<(), ProbeError> {
    if matches!(kind, K8sKind::Job) {
        let arr = v.pointer("/status/conditions").and_then(|c| c.as_array());
        if let Some(arr) = arr {
            for cond in arr {
                let Some(ct) = cond.get("type").and_then(|t| t.as_str()) else {
                    continue;
                };
                if !ct.eq_ignore_ascii_case("Failed") {
                    continue;
                }
                let status = cond
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("Unknown");
                if status.eq_ignore_ascii_case("True") {
                    let reason = cond
                        .get("reason")
                        .and_then(|s| s.as_str())
                        .unwrap_or("no reason given");
                    return Err(ProbeError::JobFailed(reason.to_owned()));
                }
            }
        }
    }
    let label = match kind {
        K8sKind::Pod => "pod",
        K8sKind::Deployment => "deployment",
        K8sKind::Job => "job",
    };
    for want in conditions {
        check_condition(v, want, label)?;
    }
    Ok(())
}

fn resource_path(kind: K8sKind, namespace: &str, name: &str) -> String {
    let ns = urlencode(namespace);
    let nm = urlencode(name);
    match kind {
        K8sKind::Pod => format!("/api/v1/namespaces/{ns}/pods/{nm}"),
        K8sKind::Deployment => format!("/apis/apps/v1/namespaces/{ns}/deployments/{nm}"),
        K8sKind::Job => format!("/apis/batch/v1/namespaces/{ns}/jobs/{nm}"),
    }
}

fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, ENCODE).to_string()
}

const ENCODE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'/')
    .add(b'?')
    .add(b'#');

fn check_ready(kind: K8sKind, v: &serde_json::Value) -> Result<(), ProbeError> {
    match kind {
        K8sKind::Pod => check_condition(v, "Ready", "pod"),
        K8sKind::Deployment => check_deployment(v),
        K8sKind::Job => check_job(v),
    }
}

fn check_job(v: &serde_json::Value) -> Result<(), ProbeError> {
    if let Some(conditions) = v.pointer("/status/conditions").and_then(|c| c.as_array()) {
        for cond in conditions {
            let Some(ct) = cond.get("type").and_then(|t| t.as_str()) else {
                continue;
            };
            if !ct.eq_ignore_ascii_case("Failed") {
                continue;
            }
            let status = cond
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("Unknown");
            if status.eq_ignore_ascii_case("True") {
                let reason = cond
                    .get("reason")
                    .and_then(|s| s.as_str())
                    .unwrap_or("no reason given");
                return Err(ProbeError::JobFailed(reason.to_owned()));
            }
        }
    }
    check_condition(v, "Complete", "job")
}

fn check_condition(v: &serde_json::Value, want: &str, label: &str) -> Result<(), ProbeError> {
    let conditions = v
        .pointer("/status/conditions")
        .and_then(|c| c.as_array())
        .ok_or_else(|| ProbeError::NotReady(format!("{label} has no `.status.conditions` yet")))?;
    for cond in conditions {
        let Some(ct) = cond.get("type").and_then(|t| t.as_str()) else {
            continue;
        };
        if ct.eq_ignore_ascii_case(want) {
            let status = cond
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("Unknown");
            if status.eq_ignore_ascii_case("True") {
                return Ok(());
            }
            let reason = cond
                .get("reason")
                .and_then(|s| s.as_str())
                .unwrap_or("no reason given");
            return Err(ProbeError::NotReady(format!(
                "{label} condition `{want}` is `{status}` ({reason})"
            )));
        }
    }
    Err(ProbeError::NotReady(format!(
        "{label} has no `{want}` condition yet"
    )))
}

fn check_deployment(v: &serde_json::Value) -> Result<(), ProbeError> {
    let generation = v
        .pointer("/metadata/generation")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let observed = v
        .pointer("/status/observedGeneration")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    if observed < generation {
        return Err(ProbeError::NotReady(format!(
            "deployment observedGeneration {observed} behind generation {generation}"
        )));
    }
    check_condition(v, "Available", "deployment")
}

struct K8sConfig {
    server: String,
    token: String,
    client: reqwest::Client,
}

fn load_config() -> Result<K8sConfig, ProbeError> {
    if let (Ok(host), Ok(port)) = (
        std::env::var("KUBERNETES_SERVICE_HOST"),
        std::env::var("KUBERNETES_SERVICE_PORT"),
    ) {
        return load_in_pod(&host, &port);
    }
    let server = std::env::var("KUBE_SERVER").map_err(|_| {
        ProbeError::NoConfig("no in-pod service account found and KUBE_SERVER is not set".into())
    })?;
    let token = std::env::var("KUBE_TOKEN")
        .map(|t| t.trim().to_owned())
        .map_err(|_| ProbeError::NoConfig("KUBE_SERVER set but KUBE_TOKEN missing".into()))?;
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("holdon/", env!("CARGO_PKG_VERSION")))
        .min_tls_version(reqwest::tls::Version::TLS_1_2);
    if let Ok(ca_path) = std::env::var("KUBE_CA_PATH") {
        let pem = std::fs::read(&ca_path)
            .map_err(|e| ProbeError::NoConfig(format!("reading KUBE_CA_PATH ({ca_path}): {e}")))?;
        let certs = Certificate::from_pem_bundle(&pem)
            .map_err(|e| ProbeError::NoConfig(format!("KUBE_CA_PATH PEM bundle invalid: {e}")))?;
        for c in certs {
            builder = builder.add_root_certificate(c);
        }
    }
    let client = builder
        .build()
        .map_err(|e| ProbeError::NoConfig(format!("building HTTP client: {e}")))?;
    Ok(K8sConfig {
        server,
        token,
        client,
    })
}

fn load_in_pod(host: &str, port: &str) -> Result<K8sConfig, ProbeError> {
    let token = std::fs::read_to_string(IN_POD_TOKEN)
        .map_err(|e| ProbeError::NoConfig(format!("reading {IN_POD_TOKEN}: {e}")))?;
    let ca_pem = std::fs::read(IN_POD_CA)
        .map_err(|e| ProbeError::NoConfig(format!("reading {IN_POD_CA}: {e}")))?;
    let certs = Certificate::from_pem_bundle(&ca_pem)
        .map_err(|e| ProbeError::NoConfig(format!("in-pod CA bundle invalid: {e}")))?;
    let host_no_brackets = host.trim_start_matches('[').trim_end_matches(']');
    let server = if host.contains(':') && !host.starts_with('[') {
        format!("https://[{host_no_brackets}]:{port}")
    } else {
        format!("https://{host}:{port}")
    };
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("holdon/", env!("CARGO_PKG_VERSION")))
        .min_tls_version(reqwest::tls::Version::TLS_1_2);
    for c in certs {
        builder = builder.add_root_certificate(c);
    }
    let client = builder
        .build()
        .map_err(|e| ProbeError::NoConfig(format!("building HTTP client: {e}")))?;
    Ok(K8sConfig {
        server,
        token: token.trim().to_owned(),
        client,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pod_ready_true() {
        let v = json!({
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "True"}
                ]
            }
        });
        assert!(check_ready(K8sKind::Pod, &v).is_ok());
    }

    #[test]
    fn pod_ready_false_returns_reason() {
        let v = json!({
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "False", "reason": "ContainersNotReady"}
                ]
            }
        });
        let err = check_ready(K8sKind::Pod, &v).unwrap_err();
        assert!(err.to_string().contains("ContainersNotReady"));
    }

    #[test]
    fn deployment_observed_generation_lag() {
        let v = json!({
            "metadata": {"generation": 5},
            "status": {"observedGeneration": 3, "conditions": [
                {"type": "Available", "status": "True"}
            ]}
        });
        let err = check_ready(K8sKind::Deployment, &v).unwrap_err();
        assert!(err.to_string().contains("observedGeneration"));
    }

    #[test]
    fn deployment_available_true_and_observed() {
        let v = json!({
            "metadata": {"generation": 1},
            "status": {"observedGeneration": 1, "conditions": [
                {"type": "Available", "status": "True"}
            ]}
        });
        assert!(check_ready(K8sKind::Deployment, &v).is_ok());
    }

    #[test]
    fn job_complete_condition_true() {
        let v = json!({
            "status": {"conditions": [
                {"type": "Complete", "status": "True"}
            ]}
        });
        assert!(check_ready(K8sKind::Job, &v).is_ok());
    }

    #[test]
    fn job_failed_condition() {
        let v = json!({
            "status": {"conditions": [
                {"type": "Failed", "status": "True", "reason": "BackoffLimitExceeded"}
            ]}
        });
        assert!(check_ready(K8sKind::Job, &v).is_err());
    }

    #[test]
    fn resource_path_shapes() {
        assert_eq!(
            resource_path(K8sKind::Pod, "default", "api"),
            "/api/v1/namespaces/default/pods/api"
        );
        assert_eq!(
            resource_path(K8sKind::Deployment, "kube-system", "core-dns"),
            "/apis/apps/v1/namespaces/kube-system/deployments/core-dns"
        );
        assert_eq!(
            resource_path(K8sKind::Job, "ci", "build-42"),
            "/apis/batch/v1/namespaces/ci/jobs/build-42"
        );
    }
}
