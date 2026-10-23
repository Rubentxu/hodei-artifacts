//! Bootstrap and IAM schema registration — integration test.
//!
//! # Why this file exists
//!
//! `tests/test_register_iam_schema_integration.rs` was a **1-byte file**
//! containing a single newline. Cargo happily compiled it into a test binary
//! that declared **zero** tests, so the suite reported "green" while asserting
//! nothing at all — a false green on the one path that decides whether the
//! service starts with a usable IAM schema.
//!
//! This file replaces it with a real assertion on the guarantees that path
//! actually carries:
//!
//! - B1 Bootstrapping with schema registration succeeds and reports the
//!   requested schema version.
//! - B2 The registered schema is **persisted**: it can be read back from
//!   storage by version.
//! - B3 (negative) Reading back a version that was never registered fails —
//!   without this, B2 would be vacuously true.
//! - B4 (negative) Bootstrapping *without* registration leaves no schema
//!   behind — the record observed by B2 is produced by the registration, not
//!   by the storage layer pre-seeding it.
//! - B5 Bootstrapping **twice** against the same database succeeds: the
//!   schema is stored under its version, so persisting it has to be
//!   idempotent. With a non-idempotent write the service could only ever
//!   boot once per database, and the second boot would fail with
//!   "Found existing record".
//! - B6 The stored schema is rebuilt **faithfully**: the schema that comes
//!   back declares the IAM entity types and actions. A schema that was
//!   persisted as something other than a schema used to come back empty, and
//!   an empty schema makes every policy that mentions a declared type look
//!   invalid.
//! - B7 (negative) A policy that references an **undeclared** entity type is
//!   rejected when a schema is registered. Without this, B6 would be
//!   vacuously true: a schema that is persisted but never applied reports
//!   every policy as valid.
//!
//! Every test owns a throwaway RocksDB directory, so no state is shared and
//! no port is bound.

use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap};
use hodei_policies::load_schema::dto::LoadSchemaCommand;
use tempfile::TempDir;

/// The schema version used by the registering tests.
const SCHEMA_VERSION: &str = "v1.0.0";

/// A policy that only uses types the IAM schema declares.
const POLICY_OVER_DECLARED_TYPES: &str =
    r#"permit(principal == Iam::User::"alice", action == Action::"CreateUser", resource);"#;

/// A policy that references an entity type no schema declares.
const POLICY_OVER_UNDECLARED_TYPE: &str =
    r#"permit(principal == Nope::Ghost::"alice", action == Action::"CreateUser", resource);"#;

/// Builds an application configuration backed by a throwaway RocksDB directory.
///
/// The returned `TempDir` must outlive the application state: dropping it
/// first would delete the database directory while the embedded engine still
/// holds it open.
fn test_config() -> (AppConfig, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");

    let mut config = AppConfig::default();
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    (config, tmp)
}

#[tokio::test]
async fn b1_bootstrap_with_registration_reports_the_requested_schema_version() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some(SCHEMA_VERSION.to_string()),
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap with IAM schema registration must succeed");

    assert_eq!(
        state.schema_version, SCHEMA_VERSION,
        "the active schema version must be the one that was registered"
    );
}

#[tokio::test]
async fn b2_the_registered_schema_is_persisted_and_readable_by_version() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some(SCHEMA_VERSION.to_string()),
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap with IAM schema registration must succeed");

    let loaded = state
        .load_schema
        .execute(LoadSchemaCommand::with_version(SCHEMA_VERSION))
        .await
        .expect("the registered schema must be readable back from storage");

    assert_eq!(
        loaded.version.as_deref(),
        Some(SCHEMA_VERSION),
        "the loaded schema must be the one that was registered"
    );
}

#[tokio::test]
async fn b3_reading_back_an_unregistered_schema_version_fails() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some(SCHEMA_VERSION.to_string()),
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap with IAM schema registration must succeed");

    let result = state
        .load_schema
        .execute(LoadSchemaCommand::with_version("v9.9.9-never-registered"))
        .await;

    assert!(
        result.is_err(),
        "a schema version that was never registered must not be readable: {result:?}"
    );
}

#[tokio::test]
async fn b4_bootstrapping_without_registration_leaves_no_schema_behind() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap without IAM schema registration must succeed");

    assert_eq!(
        state.schema_version, "unregistered",
        "an application that did not register a schema must not claim one"
    );

    let result = state
        .load_schema
        .execute(LoadSchemaCommand::with_version(SCHEMA_VERSION))
        .await;

    assert!(
        result.is_err(),
        "no schema must be readable when none was registered: {result:?}"
    );
}

/// Bootstrapping twice against the same database must succeed
///
/// The schema is persisted under its version, so persisting it has to be
/// idempotent. A non-idempotent write (a plain `create`) makes the second
/// boot fail with "Found existing record" — the service would only ever be
/// able to start once per database.
#[tokio::test]
async fn b5_bootstrapping_twice_against_the_same_database_succeeds() {
    let (config, _tmp) = test_config();

    let bootstrap_config = BootstrapConfig {
        register_iam_schema: true,
        schema_version: Some(SCHEMA_VERSION.to_string()),
        validate_schemas: false,
    };

    let first = bootstrap(&config, bootstrap_config.clone())
        .await
        .expect("the first bootstrap must succeed");

    let first_version = first.schema_version.clone();

    // The embedded database holds an exclusive lock on its directory, so the
    // first application state has to release it before the second one can
    // open the same database.
    drop(first);

    let second = bootstrap(&config, bootstrap_config)
        .await
        .expect("bootstrapping again against the same database must succeed");

    assert_eq!(
        second.schema_version, first_version,
        "the second boot must report the same schema version"
    );

    // The schema is still readable after the second boot: the second
    // registration replaced the stored schema, it did not drop it.
    let loaded = second
        .load_schema
        .execute(LoadSchemaCommand::with_version(SCHEMA_VERSION))
        .await
        .expect("the schema must remain readable after a second bootstrap");

    assert_eq!(loaded.version.as_deref(), Some(SCHEMA_VERSION));
}

/// The stored schema must come back as the schema that was stored
///
/// A schema that is persisted but cannot be rebuilt is worthless: the whole
/// point of storing it is to validate and evaluate policies against it
/// later. This asserts the rebuild is faithful — the IAM entity types and
/// actions are there, not an empty schema.
#[tokio::test]
async fn b6_the_stored_schema_is_rebuilt_with_the_declared_types() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some(SCHEMA_VERSION.to_string()),
            validate_schemas: true,
        },
    )
    .await
    .expect("bootstrap with IAM schema registration must succeed");

    let loaded = state
        .load_schema
        .execute(LoadSchemaCommand::with_version(SCHEMA_VERSION))
        .await
        .expect("the registered schema must be readable back from storage");

    assert_eq!(
        loaded.schema.entity_types().count(),
        2,
        "the rebuilt schema must declare the two IAM entity types (User, Group)"
    );

    assert_eq!(
        loaded.schema.actions().count(),
        6,
        "the rebuilt schema must declare the six IAM action types"
    );
}

/// A policy that references an undeclared type must be rejected
///
/// This is the discriminating counterpart of B6. A schema that is stored but
/// never applied reports **every** policy as valid — including one that
/// references an entity type that does not exist anywhere. That is a false
/// green at a public endpoint, so it gets its own regression.
#[tokio::test]
async fn b7_a_policy_over_an_undeclared_entity_type_is_rejected() {
    let (config, _tmp) = test_config();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some(SCHEMA_VERSION.to_string()),
            validate_schemas: true,
        },
    )
    .await
    .expect("bootstrap with IAM schema registration must succeed");

    let rejected = state
        .validate_policy
        .validate(
            hodei_policies::validate_policy::dto::ValidatePolicyCommand {
                content: POLICY_OVER_UNDECLARED_TYPE.to_string(),
            },
        )
        .await
        .expect("the validation must produce a result");

    assert!(
        !rejected.is_valid,
        "a policy that references an entity type no schema declares must not be reported as valid: {:?}",
        rejected.errors
    );

    // The counterpart: a policy that only uses declared types is accepted, so
    // the rejection above is about the undeclared type and not about the
    // validation rejecting everything.
    let accepted = state
        .validate_policy
        .validate(
            hodei_policies::validate_policy::dto::ValidatePolicyCommand {
                content: POLICY_OVER_DECLARED_TYPES.to_string(),
            },
        )
        .await
        .expect("the validation must produce a result");

    assert!(
        accepted.is_valid,
        "a policy that only uses declared types must be accepted: {:?}",
        accepted.errors
    );
}
