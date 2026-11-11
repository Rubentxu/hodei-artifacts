//! The IAM policy responses report the timestamps the store holds.
//!
//! The handlers used to fill `created_at` and `updated_at` with the clock of
//! the process that answered the request. The response therefore said when
//! the policy was created at the moment it was read, and two reads of the
//! same policy could disagree. The marks are persisted now, and this file
//! checks that what a response says is what the database holds.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};
use tempfile::TempDir;
use tower::ServiceExt;

struct Accounts;

#[async_trait]
impl PrincipalAuthenticator for Accounts {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

async fn app() -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let policy = r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#;
    let authorizer: Arc<dyn AuthorizerPort> = Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .unwrap(),
    )));
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .unwrap()
    .with_authenticator(Arc::new(Accounts))
    .with_authorization(authorizer);
    (build_router(state, &config), tmp)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer alice");
    let response = app
        .clone()
        .oneshot(match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        })
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The row the store holds for a policy.
async fn stored_row(path: &Path, policy_id: &str) -> Value {
    let db = open(path).await;
    let mut response = db
        .query("SELECT created_at, updated_at FROM policy")
        .await
        .expect("the rows must be readable");
    assert!(
        response.take_errors().is_empty(),
        "the rows must be readable without errors"
    );
    let rows: Vec<Value> = response.take(0).expect("the rows must be readable");
    assert_eq!(
        rows.len(),
        1,
        "the store must hold exactly one row for the policy `{policy_id}`"
    );
    rows.into_iter().next().expect("the row must exist")
}

/// Open a database that no other process holds.
async fn open(path: &Path) -> Surreal<Db> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                let message = error.to_string();
                let lock_contention = message.contains("lock hold by current process")
                    || message.contains("No locks available");
                if !lock_contention || std::time::Instant::now() >= deadline {
                    panic!("the database must open: {error}");
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test]
async fn the_creation_marks_are_persisted_and_the_update_moves_only_one_of_them() {
    let (app, tmp) = app().await;
    let db_path = tmp.path().join("db");

    // Create the policy.
    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "marked",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    let created_at = created["created_at"]
        .as_str()
        .expect("a creation mark")
        .to_string();
    let updated_at = created["updated_at"]
        .as_str()
        .expect("an update mark")
        .to_string();
    assert_eq!(created_at, updated_at, "a new policy was not updated yet");

    // Update the policy.
    let (status, updated) = call(
        &app,
        "PUT",
        "/api/v1/iam/policies/update",
        Some(json!({
            "policy_hrn": created["hrn"],
            "policy_content": "permit(principal, action == Admin::Action::\"AccessAdminApi\", resource);"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update: {updated:?}");

    let updated_created_at = updated["created_at"]
        .as_str()
        .expect("a creation mark")
        .to_string();
    let updated_updated_at = updated["updated_at"]
        .as_str()
        .expect("an update mark")
        .to_string();

    assert_eq!(
        updated_created_at, created_at,
        "the creation mark must not change when the policy is updated"
    );
    assert_ne!(
        updated_updated_at, updated_at,
        "the update mark must move when the policy is updated"
    );

    // The store holds the new marks, and the read reports them.
    //
    // The service is released first: the embedded database holds an exclusive
    // lock while it is open, and the row can only be read by a process that
    // does not hold it.
    drop(app);
    let row = stored_row(&db_path, "acme~marked").await;
    assert_eq!(
        row["created_at"].as_str(),
        Some(updated_created_at.as_str())
    );
    assert_eq!(
        row["updated_at"].as_str(),
        Some(updated_updated_at.as_str())
    );
}

#[tokio::test]
async fn the_read_and_the_listing_report_the_stored_marks() {
    let (app, tmp) = app().await;
    let db_path = tmp.path().join("db");

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "readable",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    // The read reports what the store holds.
    let (status, read) = call(
        &app,
        "POST",
        "/api/v1/iam/policies/get",
        Some(json!({"policy_hrn": created["hrn"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "read: {read:?}");
    let read_created = read["created_at"]
        .as_str()
        .expect("a creation mark")
        .to_string();
    let read_updated = read["updated_at"]
        .as_str()
        .expect("an update mark")
        .to_string();
    assert!(!read_created.is_empty() && !read_updated.is_empty());

    // And so does the listing.
    let (status, listed) = call(&app, "GET", "/api/v1/iam/policies", None).await;
    assert_eq!(status, StatusCode::OK, "list: {listed:?}");
    let summary = listed["policies"]
        .as_array()
        .expect("the listing has policies")
        .iter()
        .find(|policy| policy["name"] == "readable")
        .expect("the policy must be listed")
        .clone();
    let listed_created = summary["created_at"]
        .as_str()
        .expect("a creation mark")
        .to_string();
    let listed_updated = summary["updated_at"]
        .as_str()
        .expect("an update mark")
        .to_string();

    // And the store holds exactly those marks.
    drop(app);
    let row = stored_row(&db_path, "acme~readable").await;
    assert_eq!(row["created_at"].as_str(), Some(listed_created.as_str()));
    assert_eq!(row["updated_at"].as_str(), Some(listed_updated.as_str()));
    assert_eq!(
        Some(listed_created.as_str()),
        read["created_at"].as_str(),
        "the read and the listing must report the same creation mark"
    );
    assert_eq!(
        Some(listed_updated.as_str()),
        read["updated_at"].as_str(),
        "the read and the listing must report the same update mark"
    );
}
