use url::Url;

use super::AttemptCtx;
use crate::diagnostic::{Stage, StageKind};

const TEMPORAL_SERVICE: &str = "temporal.api.workflowservice.v1.WorkflowService";

pub(super) async fn probe(url: &Url, ctx: AttemptCtx) -> Vec<Stage> {
    let grpc_url = rewrite_url(url);
    let mut stages = super::grpc::probe(&grpc_url, TEMPORAL_SERVICE, ctx).await;
    for stage in &mut stages {
        stage.kind = StageKind::Temporal;
    }
    stages
}

fn rewrite_url(url: &Url) -> Url {
    let raw = url.as_str();
    let rewritten = if let Some(rest) = raw.strip_prefix("temporal://") {
        format!("grpc://{rest}")
    } else if let Some(rest) = raw.strip_prefix("temporals://") {
        format!("grpcs://{rest}")
    } else {
        return url.clone();
    };
    Url::parse(&rewritten).unwrap_or_else(|_| url.clone())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_temporal_to_grpc() {
        let u: Url = "temporal://h:7233".parse().unwrap();
        let out = rewrite_url(&u);
        assert_eq!(out.scheme(), "grpc");
    }

    #[test]
    fn rewrite_temporals_to_grpcs() {
        let u: Url = "temporals://h:7233".parse().unwrap();
        let out = rewrite_url(&u);
        assert_eq!(out.scheme(), "grpcs");
    }

    #[test]
    fn rewrite_preserves_host_and_port() {
        let u: Url = "temporal://example.com:7233".parse().unwrap();
        let out = rewrite_url(&u);
        assert_eq!(out.host_str(), Some("example.com"));
        assert_eq!(out.port(), Some(7233));
    }
}
