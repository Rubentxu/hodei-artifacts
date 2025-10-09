//! Bootstrap module for Hodei Artifacts API
//!
//! This module handles the initialization of the application, including:
//! - Database connection setup
//! - Infrastructure adapter creation
//! - Use case composition via CompositionRoot
//! - Optional IAM schema registration

use crate::app_state::AppState;
use crate::composition_root::CompositionRoot;
use async_trait::async_trait;
use hodei_iam::features::register_iam_schema::dto::{
    RegisterIamSchemaCommand, RegisterIamSchemaResult,
};
use hodei_policies::features::build_schema::error::BuildSchemaError;
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::remote::ws::{Client, Ws};
use surrealdb::opt::auth::Root;
use tracing::{error, info, warn};

/// Configuration for the bootstrap process
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Whether to register the IAM schema on startup
    pub register_iam_schema: bool,
    /// Optional schema version to use
    pub schema_version: Option<String>,
    /// Whether to validate schemas during registration
    pub validate_schemas: bool,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            register_iam_schema: true,
            schema_version: Some("v1.0.0".to_string()),
            validate_schemas: true,
        }
    }
}

/// Bootstrap error types
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("Failed to connect to database: {0}")]
    DatabaseConnection(String),

    #[error("Failed to authenticate: {0}")]
    Authentication(String),

    #[error("Failed to initialize namespace/database: {0}")]
    Initialization(String),

    #[error("Failed to register IAM schema: {0}")]
    SchemaRegistration(String),
}

/// Bootstrap the application with the given configuration
///
/// This function:
/// 1. Initializes the database connection and storage adapters
/// 2. Creates the CompositionRoot with all use case ports
/// 3. Optionally registers the IAM schema
/// 4. Returns the configured AppState ready for Axum
pub async fn bootstrap(
    config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Hodei Artifacts API bootstrap");

    // Step 1: Initialize infrastructure
    info!("📦 Initializing infrastructure adapters");
    let schema_storage = initialize_schema_storage().await?;

    // Step 2: Use Composition Root to create all use case ports
    info!("🏗️  Creating use cases via CompositionRoot");
    let root = CompositionRoot::production(schema_storage.clone());

    // Step 3: Determine schema version
    let schema_version = if config.register_iam_schema {
        info!("📝 Registering IAM schema");
        let result = register_iam_schema(
            &*root.iam_ports.register_iam_schema,
            config.schema_version.clone(),
            config.validate_schemas,
        )
        .await?;

        info!(
            "✅ IAM schema registered successfully (version: {}, entities: {}, actions: {})",
            result.schema_version, result.entity_types_registered, result.action_types_registered
        );

        result.schema_version
    } else {
        warn!("⚠️  Skipping IAM schema registration");
        config
            .schema_version
            .unwrap_or_else(|| "unregistered".to_string())
    };

    // Step 4: Create AppState from CompositionRoot
    info!("🎯 Creating application state");
    let app_state = AppState::from_composition_root(schema_version.clone(), root);

    info!(
        "✅ Bootstrap completed successfully (schema version: {})",
        schema_version
    );

    Ok(app_state)
}

/// SurrealDB adapter for schema storage
///
/// This adapter implements the SchemaStoragePort trait for SurrealDB.
#[derive(Clone)]
pub struct SurrealSchemaAdapter {
    db: Surreal<Client>,
}

impl SurrealSchemaAdapter {
    /// Get a reference to the underlying database client
    pub fn db(&self) -> &Surreal<Client> {
        &self.db
    }
}

impl SurrealSchemaAdapter {
    /// Create a new SurrealDB schema adapter
    pub fn new(db: Surreal<Client>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SchemaStoragePort for SurrealSchemaAdapter {
    async fn save_schema(
        &self,
        schema_json: String,
        version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        // Generate a unique schema ID
        let schema_id = version.clone().unwrap_or_else(|| "latest".to_string());

        // Save the schema to SurrealDB
        let _: Option<serde_json::Value> = self
            .db
            .create(("schema", schema_id.as_str()))
            .content(serde_json::json!({
                "content": schema_json,
                "version": version,
                "created_at": chrono::Utc::now().to_rfc3339(),
            }))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(format!("schema:{}", schema_id))
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        let result: Option<serde_json::Value> = self
            .db
            .select(("schema", "latest"))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.and_then(|v| v.get("content").and_then(|c| c.as_str().map(String::from))))
    }

    async fn get_schema_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        let result: Option<serde_json::Value> = self
            .db
            .select(("schema", version))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.and_then(|v| v.get("content").and_then(|c| c.as_str().map(String::from))))
    }

    async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError> {
        let result: Option<serde_json::Value> = self
            .db
            .delete(("schema", schema_id))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.is_some())
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        // Query all schemas
        let results: Vec<serde_json::Value> = self
            .db
            .query("SELECT version FROM schema")
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?
            .take(0)
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        let versions = results
            .into_iter()
            .filter_map(|v| {
                v.get("version")
                    .and_then(|ver| ver.as_str().map(String::from))
            })
            .collect();

        Ok(versions)
    }
}

/// Initialize the SurrealDB schema storage adapter
async fn initialize_schema_storage()
-> Result<Arc<SurrealSchemaAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "127.0.0.1:8000".to_string());
    let db_user = std::env::var("DATABASE_USER").unwrap_or_else(|_| "root".to_string());
    let db_pass = std::env::var("DATABASE_PASS").unwrap_or_else(|_| "root".to_string());
    let db_namespace = std::env::var("DATABASE_NS").unwrap_or_else(|_| "hodei".to_string());
    let db_name = std::env::var("DATABASE_DB").unwrap_or_else(|_| "artifacts".to_string());

    info!("🔌 Connecting to SurrealDB at {}", db_url);

    let db = Surreal::new::<Ws>(db_url)
        .await
        .map_err(|e| BootstrapError::DatabaseConnection(e.to_string()))?;

    info!("🔑 Authenticating with SurrealDB");
    db.signin(Root {
        username: &db_user,
        password: &db_pass,
    })
    .await
    .map_err(|e| BootstrapError::Authentication(e.to_string()))?;

    info!(
        "📂 Using namespace '{}' and database '{}'",
        db_namespace, db_name
    );
    db.use_ns(&db_namespace)
        .use_db(&db_name)
        .await
        .map_err(|e| BootstrapError::Initialization(e.to_string()))?;

    Ok(Arc::new(SurrealSchemaAdapter::new(db)))
}

/// Register the IAM schema using the provided use case
async fn register_iam_schema(
    use_case: &dyn hodei_iam::features::register_iam_schema::ports::RegisterIamSchemaPort,
    version: Option<String>,
    validate: bool,
) -> Result<RegisterIamSchemaResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut command = RegisterIamSchemaCommand::new().with_validation(validate);

    if let Some(v) = version {
        command = command.with_version(v);
    }

    let result = use_case
        .register(command)
        .await
        .map_err(|e| BootstrapError::SchemaRegistration(e.to_string()))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires SurrealDB instance"]
    async fn test_bootstrap_with_default_config() {
        let config = BootstrapConfig::default();
        let result = bootstrap(config).await;

        // Should succeed if SurrealDB is available
        if result.is_ok() {
            println!("Bootstrap succeeded");
        } else {
            println!("Bootstrap failed (expected if no DB): {:?}", result.err());
        }
    }

    #[tokio::test]
    #[ignore = "Requires SurrealDB instance"]
    async fn test_bootstrap_without_iam_schema_registration() {
        let config = BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(config).await;

        if result.is_ok() {
            println!("Bootstrap succeeded without IAM schema");
        } else {
            println!("Bootstrap failed (expected if no DB): {:?}", result.err());
        }
    }

    #[tokio::test]
    #[ignore = "Requires SurrealDB instance"]
    async fn test_bootstrap_with_custom_schema_version() {
        let config = BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some("v2.0.0-test".to_string()),
            validate_schemas: true,
        };

        let result = bootstrap(config).await;

        if let Ok(app_state) = result {
            assert_eq!(app_state.schema_version, "v2.0.0-test");
        }
    }
}
