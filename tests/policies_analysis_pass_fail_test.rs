use axum::{body::{Body, to_bytes}, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn analysis_pass_with_mfa_required() {
    let app = build_test_app().await;

    let body = json!({
        "policies": ["permit(principal, action, resource) when { context.mfa == true };"],
        "schema": null,
        "rules": [
            {"id": "r1", "kind": "no_permit_without_mfa", "params": {}}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/analysis")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["passed"].as_bool().unwrap(), true);
    assert_eq!(v["violations"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn analysis_fail_without_mfa_condition() {
    let app = build_test_app().await;

    let body = json!({
        "policies": ["permit(principal, action, resource);"],
        "schema": null,
        "rules": [
            {"id": "r1", "kind": "no_permit_without_mfa", "params": {}}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/analysis")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["passed"].as_bool().unwrap(), false);
    assert!(v["violations"].as_array().unwrap().len() > 0);
}
