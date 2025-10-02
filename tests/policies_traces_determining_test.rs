use axum::{body::Body, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
#[ignore] // TODO: Fix duplicate policy ID issue when cloning policies in parallel workers
async fn playground_traces_returns_determining_policies() {
    let app = build_test_app().await;

    // Policies: forbid admins; permit all. Entities: alice in admins
    let body = json!({
        "policies": [
            "@id(\"forbid-admins\") forbid(principal in Group::\"admins\", action, resource);",
            "@id(\"permit-all\") permit(principal, action, resource);"
        ],
        "schema": null,
        "entities": [
            {"uid":"User::\"alice\"","attributes":{},"parents":["Group::\"admins\""]},
            {"uid":"Group::\"admins\"","attributes":{},"parents":[]}
        ],
        "authorization_requests": [
            {"name":"alice-deny","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": null}
        ],
        "options": {"include_diagnostics": true, "include_policy_traces": true}
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    if status != StatusCode::OK {
        let body_str = String::from_utf8_lossy(&bytes);
        eprintln!("Response status: {}, body: {}", status, body_str);
    }
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let auth = v["authorization_results"].as_array().unwrap();
    let det = &auth[0]["determining_policies"];
    assert!(det.is_array());
    assert!(det.as_array().unwrap().len() >= 1);
}
