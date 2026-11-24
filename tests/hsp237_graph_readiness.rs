//! HSP-237 (graph profile): the readiness of the Dgraph profile depends on
//! the alpha. Deterministic, no Dgraph: a fake `GraphHealthPort` fails and
//! recovers, and the real router reports `503` / `200` accordingly while the
//! liveness stays `200`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use hodei_artifacts_api::readiness::{
    CachedProbe, ContentCheck, Dependency, DependencyFailure, ProfileProbe, ReadinessProbe,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_graph_client::{GraphClientError, GraphHealthPort, GraphReadiness};
use tower::ServiceExt;

/// What the fake alpha answers.
#[derive(Clone, Copy)]
enum Mode {
    Up,
    Down,
    LoginRejected,
    NoAuthenticatedRead,
    Slow,
}

struct FakeGraph(Mutex<Mode>);

impl FakeGraph {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self(Mutex::new(mode)))
    }

    fn set(&self, mode: Mode) {
        *self.0.lock().expect("the mode") = mode;
    }
}

#[async_trait]
impl GraphHealthPort for FakeGraph {
    async fn ready(&self) -> Result<GraphReadiness, GraphClientError> {
        let mode = *self.0.lock().expect("the mode");
        match mode {
            Mode::Up => Ok(GraphReadiness {
                server_version: Some("v25".to_string()),
                authenticated_query_served: true,
                login_performed: true,
            }),
            Mode::Down => Err(GraphClientError::Request {
                phase: "query",
                source: tonic::Status::unavailable("tcp connect error 10.0.0.1:9080 secret-token"),
            }),
            Mode::LoginRejected => Err(GraphClientError::LoginRejected {
                userid: "groot".to_string(),
                detail: "invalid username or password".to_string(),
            }),
            Mode::NoAuthenticatedRead => Ok(GraphReadiness {
                server_version: None,
                authenticated_query_served: false,
                login_performed: true,
            }),
            Mode::Slow => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                unreachable!("the probe budget cuts the check first")
            }
        }
    }
}

fn graph_probe(graph: Arc<FakeGraph>) -> ProfileProbe {
    ProfileProbe::new().with_graph(graph)
}

async fn app(probe: Arc<dyn ReadinessProbe>) -> (Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let mut config = AppConfig::default();
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();
    config.content.cas_path = tmp.path().join("cas").to_string_lossy().into_owned();
    config.repository.durable = true;
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the bootstrap")
    .with_readiness(probe);
    (build_router(state, &config), tmp)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("the request");
    let response = app.clone().oneshot(request).await.expect("the router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn the_graph_is_a_required_dependency_of_the_profile() {
    let probe = graph_probe(FakeGraph::new(Mode::Up));
    assert_eq!(probe.required(), vec![Dependency::Graph]);
    assert!(probe.report().await.ready);
}

#[tokio::test]
async fn the_failure_class_follows_the_cause() {
    let graph = FakeGraph::new(Mode::Down);
    let probe = graph_probe(graph.clone());
    let cases = [
        (Mode::Down, DependencyFailure::Unreachable),
        (Mode::LoginRejected, DependencyFailure::Misconfigured),
        (Mode::NoAuthenticatedRead, DependencyFailure::Unreachable),
        (Mode::Slow, DependencyFailure::Timeout),
    ];
    for (mode, expected) in cases {
        graph.set(mode);
        let report = probe.report().await;
        assert!(!report.ready);
        assert_eq!(report.failing, vec![(Dependency::Graph, expected)]);
    }
}

#[tokio::test]
async fn the_content_store_is_still_checked_next_to_the_graph() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let probe = graph_probe(FakeGraph::new(Mode::Up)).with_content(ContentCheck::Filesystem {
        root: tmp.path().join("missing"),
    });
    let report = probe.report().await;
    assert_eq!(
        report.failing,
        vec![(Dependency::ContentStore, DependencyFailure::Unreachable)]
    );
}

#[tokio::test]
async fn ready_follows_the_alpha_and_live_stays_200() {
    let graph = FakeGraph::new(Mode::Up);
    let probe = Arc::new(CachedProbe::with_ttl(
        Arc::new(graph_probe(graph.clone())),
        Duration::ZERO,
    ));
    let (app, _tmp) = app(probe).await;

    let (status, _) = get(&app, "/health/ready").await;
    assert_eq!(status, StatusCode::OK);

    graph.set(Mode::Down);
    let (status, body) = get(&app, "/health/ready").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        body.contains("graph"),
        "the body names the dependency: {body}"
    );
    assert!(body.contains("unreachable"), "{body}");
    for secret in ["10.0.0.1", "secret-token", "9080", "groot"] {
        assert!(!body.contains(secret), "the body leaks {secret}: {body}");
    }
    let (live, _) = get(&app, "/health/live").await;
    assert_eq!(live, StatusCode::OK);

    graph.set(Mode::LoginRejected);
    let (status, body) = get(&app, "/health/ready").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("misconfigured"), "{body}");
    assert!(
        !body.contains("groot") && !body.contains("password"),
        "{body}"
    );

    graph.set(Mode::Up);
    let (status, _) = get(&app, "/health/ready").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_graph_failure_is_cached_for_the_short_ttl() {
    let graph = FakeGraph::new(Mode::Up);
    let cached = CachedProbe::new(Arc::new(graph_probe(graph.clone())));
    assert!(cached.report().await.ready);
    graph.set(Mode::Down);
    // Within the one-second TTL the verdict is the cached one.
    assert!(cached.report().await.ready);
}
