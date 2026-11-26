//! HSP-292: the HTTP timeout of `/health/ready`, through the real router.
//!
//! A `ReadinessProbe` whose dependency never answers must not hang the
//! request: the router answers `503` in less than 2.5 s (the probe budget is
//! 2 s, a dependency budget 1 s) and the body names the `timeout`. This needs
//! no Dgraph and no S3: the silent probe is injected into the state that
//! `build_router` serves.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use hodei_artifacts_api::readiness::{
    CachedProbe, Dependency, DependencyFailure, ReadinessProbe, VERDICT_TTL,
};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use serde_json::Value;
use tower::ServiceExt;

// The graph profile of the harness: a REAL Dgraph namespace per application.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

/// The wall-clock ceiling of one readiness request.
const CEILING: Duration = Duration::from_millis(2500);

/// A probe whose required dependencies never answer.
struct Silent {
    required: Vec<Dependency>,
}

#[async_trait]
impl ReadinessProbe for Silent {
    fn required(&self) -> Vec<Dependency> {
        self.required.clone()
    }

    async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(())
    }
}

async fn the_app(probe: Arc<dyn ReadinessProbe>) -> (Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    tmp.apply(&mut config);
    config.content.cas_path = tmp.path().join("cas").to_string_lossy().into_owned();
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

async fn get(app: &Router, uri: &str) -> (StatusCode, Value, Duration) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("the request");
    let started = Instant::now();
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let elapsed = started.elapsed();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the body");
    let body = serde_json::from_slice(&bytes).expect("a JSON body");
    (status, body, elapsed)
}

fn statuses(body: &Value) -> Vec<(String, String)> {
    body["dependencies"]
        .as_array()
        .expect("the dependencies array")
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap().to_string(),
                entry["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_silent_dependency_gives_503_with_timeout_in_under_two_and_a_half_seconds() {
    let probe = Arc::new(CachedProbe::new(Arc::new(Silent {
        required: vec![Dependency::ContentStore],
    })));
    let (app, _tmp) = the_app(probe).await;

    let (status, body, elapsed) = get(&app, "/health/ready").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["status"], "not_ready");
    assert_eq!(
        statuses(&body),
        vec![("content_store".to_string(), "timeout".to_string())]
    );
    assert!(
        elapsed < CEILING,
        "the readiness hung for {elapsed:?} (ceiling {CEILING:?})"
    );
}

#[tokio::test]
async fn three_silent_dependencies_share_the_two_second_probe_budget() {
    // 1 s for the first, 1 s for the second, and the third no longer fits in
    // the 2 s probe budget: it is declared `timeout` without being asked.
    let probe = Arc::new(CachedProbe::new(Arc::new(Silent {
        required: vec![
            Dependency::TrustPolicy,
            Dependency::Graph,
            Dependency::ContentStore,
        ],
    })));
    let (app, _tmp) = the_app(probe).await;

    let (status, body, elapsed) = get(&app, "/health/ready").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(
        statuses(&body),
        vec![
            ("trust_policy".to_string(), "timeout".to_string()),
            ("graph".to_string(), "timeout".to_string()),
            ("content_store".to_string(), "timeout".to_string()),
        ]
    );
    assert!(
        elapsed < CEILING,
        "three silent dependencies took {elapsed:?} (ceiling {CEILING:?})"
    );
    assert!(
        elapsed >= Duration::from_millis(1900),
        "the first two dependencies must really spend their 1 s budget, took {elapsed:?}"
    );
}

#[tokio::test]
async fn the_liveness_does_not_wait_for_the_silent_dependency() {
    let probe = Arc::new(CachedProbe::new(Arc::new(Silent {
        required: vec![Dependency::ContentStore],
    })));
    let (app, _tmp) = the_app(probe).await;

    let (status, body, elapsed) = get(&app, "/health/live").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "alive");
    assert!(
        elapsed < Duration::from_millis(500),
        "live took {elapsed:?}"
    );
}

#[test]
fn the_verdict_ttl_is_one_second() {
    assert_eq!(VERDICT_TTL, Duration::from_secs(1));
}
