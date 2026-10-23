//! The persisted schema has to be found again by the reader that applies it.
//!
//! # Why this file exists
//!
//! The schema was persisted, but the record was written with a `null` version
//! whenever the registration did not name one. The reader looks the newest
//! record up by its version, so such a record was never selected: the schema
//! existed in the database and was nonetheless invisible to every consumer.
//!
//! The visible consequence was not a missing schema but a *wrong* one: with
//! no schema found, policy validation degraded to a syntax check, so a policy
//! over an undeclared action or an undeclared entity type was reported as
//! valid. The service accepted policies it had to reject.
//!
//! # Guarantees
//!
//! - S1 A schema stored without a version is found again by the reader.
//! - S2 A schema stored with a version is found again by the reader.
//! - S3 After the IAM schema is registered, a policy over a registered action
//!   is valid and a policy over an undeclared action or entity type is
//!   rejected. This is the discriminating counterpart: it only holds when the
//!   schema that was persisted is the schema that is applied.
//!
//! Each test owns a throwaway database directory, so no state is shared.

use hodei_artifacts_api::bootstrap::SurrealSchemaAdapter;
use hodei_artifacts_api::composition_root::CompositionRoot;
use hodei_iam::features::register_iam_schema::dto::RegisterIamSchemaCommand;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::build_schema::ports::SchemaStoragePort;
use hodei_policies::validate_policy::dto::ValidatePolicyCommand;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// Opens an embedded database on a throwaway directory and returns its client.
async fn open_database(path: &std::path::Path) -> Surreal<Db> {
    let db = Surreal::new::<RocksDb>(path)
        .await
        .expect("the embedded database must open");
    db.use_ns("hodei")
        .use_db("artifacts")
        .await
        .expect("the namespace and database must be selected");
    db
}

/// S1: a schema stored without a version is found again
#[tokio::test]
async fn s1_a_schema_stored_without_a_version_is_found_again() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_database(&tmp.path().join("db")).await;
    let storage = SurrealSchemaAdapter::new(db);

    storage
        .save_schema("envelope-content".to_string(), None)
        .await
        .expect("the schema must be stored");

    let found = storage
        .get_latest_schema()
        .await
        .expect("the reader must not fail");

    assert_eq!(
        found.as_deref(),
        Some("envelope-content"),
        "a schema stored without a version must be found again by the reader"
    );
}

/// S2: a schema stored with a version is found again
#[tokio::test]
async fn s2_a_schema_stored_with_a_version_is_found_again() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_database(&tmp.path().join("db")).await;
    let storage = SurrealSchemaAdapter::new(db);

    storage
        .save_schema("versioned-content".to_string(), Some("v1.0.0".to_string()))
        .await
        .expect("the schema must be stored");

    let found = storage
        .get_latest_schema()
        .await
        .expect("the reader must not fail");

    assert_eq!(
        found.as_deref(),
        Some("versioned-content"),
        "a schema stored with a version must be found again by the reader"
    );
}

/// S3: the registered schema is the schema that validates the policies
#[tokio::test]
async fn s3_the_registered_schema_is_the_schema_that_validates() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_database(&tmp.path().join("db")).await;

    let storage = Arc::new(SurrealSchemaAdapter::new(db.clone()));
    let policy_adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db)));
    let root = CompositionRoot::production(storage, policy_adapter);

    // The registration does not name a version: this is the path that used to
    // persist a schema nobody could find.
    root.iam_ports
        .register_iam_schema
        .register(RegisterIamSchemaCommand::new().with_validation(true))
        .await
        .expect("the IAM schema must be registered");

    let validator = root.policy_ports.validate_policy.clone();
    let validate = |content: &str| {
        let validator = validator.clone();
        let content = content.to_string();
        async move {
            validator
                .validate(ValidatePolicyCommand { content })
                .await
                .expect("the validation must not fail")
        }
    };

    let declared =
        validate(r#"permit(principal, action == Action::"CreateUser", resource);"#).await;
    assert!(
        declared.is_valid,
        "a policy over a registered action must be accepted: {:?}",
        declared.errors
    );

    let undeclared_action =
        validate(r#"permit(principal, action == Action::"NoSuchAction", resource);"#).await;
    assert!(
        !undeclared_action.is_valid,
        "a policy over an undeclared action must be rejected: {:?}",
        undeclared_action.errors
    );

    let undeclared_entity =
        validate(r#"permit(principal == Nope::Ghost::"alice", action, resource);"#).await;
    assert!(
        !undeclared_entity.is_valid,
        "a policy over an undeclared entity type must be rejected: {:?}",
        undeclared_entity.errors
    );
}
