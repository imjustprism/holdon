use std::time::Instant;

use prost::Message;
use tonic::client::Grpc;
use tonic::codec::ProstCodec;
use tonic::transport::{ClientTlsConfig, Endpoint};
use tonic::{Code, Request, Status};
use url::Url;

use super::hint::hints;
use super::{AttemptCtx, err_stage, ok_stage};
use crate::diagnostic::{Stage, StageKind};

const HEALTH_PATH: &str = "/grpc.health.v1.Health/Check";

#[derive(Clone, PartialEq, Message)]
struct HealthCheckRequest {
    #[prost(string, tag = "1")]
    service: ::prost::alloc::string::String,
}

#[derive(Clone, PartialEq, Message)]
struct HealthCheckResponse {
    #[prost(enumeration = "ServingStatus", tag = "1")]
    status: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ::prost::Enumeration)]
#[repr(i32)]
enum ServingStatus {
    Unknown = 0,
    Serving = 1,
    NotServing = 2,
    ServiceUnknown = 3,
}

pub(super) async fn probe(url: &Url, service: &str, ctx: AttemptCtx) -> Vec<Stage> {
    let start = Instant::now();
    match probe_inner(url, service, ctx).await {
        Ok(()) => vec![ok_stage(StageKind::Grpc, start.elapsed())],
        Err(ProbeError::Connect(e)) => vec![err_stage(
            StageKind::Grpc,
            start.elapsed(),
            format!("connect: {e}"),
            Some(hints::PORT_CLOSED),
        )],
        Err(ProbeError::Tls(e)) => vec![err_stage(
            StageKind::Grpc,
            start.elapsed(),
            format!("tls config: {e}"),
            Some(hints::GRPC_TLS),
        )],
        Err(ProbeError::Endpoint(e)) => vec![err_stage(
            StageKind::Grpc,
            start.elapsed(),
            format!("endpoint: {e}"),
            None,
        )],
        Err(ProbeError::Rpc(status)) => vec![rpc_failure_stage(&status, start)],
        Err(ProbeError::NotServing(name)) => vec![err_stage(
            StageKind::Grpc,
            start.elapsed(),
            if name.is_empty() {
                "health status NOT_SERVING (overall server)".to_owned()
            } else {
                format!("health status NOT_SERVING for service `{name}`")
            },
            Some(hints::GRPC_NOT_SERVING),
        )],
        Err(ProbeError::UnknownStatus(name, status)) => vec![err_stage(
            StageKind::Grpc,
            start.elapsed(),
            format!("unexpected health status {status} for service `{name}`"),
            Some(hints::GRPC_NOT_SERVING),
        )],
    }
}

fn rpc_failure_stage(status: &Status, start: Instant) -> Stage {
    let hint = match status.code() {
        Code::Unimplemented => Some(hints::GRPC_UNIMPLEMENTED),
        Code::NotFound => Some(hints::GRPC_SERVICE_UNKNOWN),
        Code::Unavailable => Some(hints::GRPC_UNAVAILABLE),
        Code::DeadlineExceeded => Some(hints::GRPC_DEADLINE),
        Code::Unauthenticated | Code::PermissionDenied => Some(hints::GRPC_AUTH),
        _ => None,
    };
    err_stage(
        StageKind::Grpc,
        start.elapsed(),
        format!("rpc {:?}: {}", status.code(), status.message()),
        hint,
    )
}

enum ProbeError {
    Endpoint(Box<tonic::transport::Error>),
    Tls(Box<tonic::transport::Error>),
    Connect(Box<tonic::transport::Error>),
    Rpc(Box<Status>),
    NotServing(String),
    UnknownStatus(String, i32),
}

async fn probe_inner(url: &Url, service: &str, ctx: AttemptCtx) -> Result<(), ProbeError> {
    let endpoint = build_endpoint(url, ctx)?;
    let channel = endpoint
        .connect()
        .await
        .map_err(|e| ProbeError::Connect(Box::new(e)))?;

    let mut client = Grpc::new(channel);
    client
        .ready()
        .await
        .map_err(|e| ProbeError::Rpc(Box::new(Status::unknown(e.to_string()))))?;

    let codec: ProstCodec<HealthCheckRequest, HealthCheckResponse> = ProstCodec::default();
    let path = http::uri::PathAndQuery::from_static(HEALTH_PATH);
    let req = Request::new(HealthCheckRequest {
        service: service.to_owned(),
    });
    let response = client
        .unary(req, path, codec)
        .await
        .map_err(|s| ProbeError::Rpc(Box::new(s)))?;

    let body = response.into_inner();
    match ServingStatus::try_from(body.status) {
        Ok(ServingStatus::Serving) => Ok(()),
        Ok(ServingStatus::NotServing) => Err(ProbeError::NotServing(service.to_owned())),
        Ok(ServingStatus::ServiceUnknown) => Err(ProbeError::Rpc(Box::new(Status::not_found(
            format!("service `{service}` unknown to health server"),
        )))),
        Ok(ServingStatus::Unknown) | Err(_) => {
            Err(ProbeError::UnknownStatus(service.to_owned(), body.status))
        }
    }
}

fn build_endpoint(url: &Url, ctx: AttemptCtx) -> Result<Endpoint, ProbeError> {
    let want_tls = url.scheme() == "grpcs";
    let host = url
        .host_str()
        .ok_or_else(|| ProbeError::Rpc(Box::new(Status::invalid_argument("missing host"))))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProbeError::Rpc(Box::new(Status::invalid_argument("missing port"))))?;
    let scheme = if want_tls { "https" } else { "http" };
    let uri = format!("{scheme}://{host}:{port}");
    let mut endpoint = Endpoint::try_from(uri)
        .map_err(|e| ProbeError::Endpoint(Box::new(e)))?
        .connect_timeout(ctx.attempt_timeout)
        .timeout(ctx.attempt_timeout);
    if want_tls {
        let tls = ClientTlsConfig::new()
            .with_webpki_roots()
            .domain_name(host.to_owned());
        endpoint = endpoint
            .tls_config(tls)
            .map_err(|e| ProbeError::Tls(Box::new(e)))?;
    }
    Ok(endpoint)
}
