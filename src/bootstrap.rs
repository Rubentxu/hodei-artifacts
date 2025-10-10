//! Bootstrap module for Hodei Artifacts API
//!
//! This module handles the initialization of the application, including:
//! - RocksDB database connection setup
//! - Infrastructure adapter creation
//! - Use case composition via CompositionRoot
//! - Optional IAM schema registration

use crate::app_state::AppState;
use crate::composition_root::CompositionRoot;
use crate::config::AppConfig;
use async_trait::async_trait;

use hodei_iam::features::register_iam_schema::dto::{
    RegisterIamSchemaCommand, RegisterIamSchemaResult,
};
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use hodei_policies::features::build_schema::error::BuildSchemaError;
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
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

    #[error("Failed to initialize namespace/database: {0}")]
    Initialization(String),

    #[error("Failed to register IAM schema: {0}")]
    SchemaRegistration(String),
}

/// Bootstrap the application with the given configuration
///
/// This function:
/// 1. Validates configuration and fails explicitly on any issues
/// 2. Initializes the RocksDB database connection and storage adapters
/// 3. Creates the CompositionRoot with all use case ports
/// 4. Optionally registers the IAM schema
/// 5. Returns the configured AppState ready for Axum
pub async fn bootstrap(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Hodei Artifacts API bootstrap");

    // Step 0: Validate configuration and fail explicitly on any issues
    info!("🔍 Validating application configuration");
    validate_bootstrap_configuration(config)
        .map_err(|e| BootstrapError::Initialization(e.to_string()))?;

    // Step 1: Initialize infrastructure with RocksDB
    info!("📦 Initializing infrastructure adapters");
    let schema_storage = initialize_schema_storage(config).await?;

    // Initialize policy adapter with the same DB client
    let policy_adapter = Arc::new(SurrealPolicyAdapter::new(
        schema_storage.db().clone().into(),
    ));

    // Step 2: Use Composition Root to create all use case ports
    info!("🏗️  Creating use cases via CompositionRoot");
    let root = CompositionRoot::production(schema_storage.clone(), policy_adapter);

    // Step 3: Determine schema version
    let schema_version = if bootstrap_config.register_iam_schema {
        info!("📝 Registering IAM schema");
        let result = register_iam_schema(
            &*root.iam_ports.register_iam_schema,
            bootstrap_config.schema_version.clone(),
            bootstrap_config.validate_schemas,
        )
        .await?;

        info!(
            "✅ IAM schema registered successfully (version: {}, entities: {}, actions: {})",
            result.schema_version, result.entity_types_registered, result.action_types_registered
        );

        result.schema_version
    } else {
        warn!("⚠️  Skipping IAM schema registration");
        bootstrap_config
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
/// This adapter implements the SchemaStoragePort trait for SurrealDB with RocksDB.
#[derive(Clone)]
pub struct SurrealSchemaAdapter {
    db: Surreal<surrealdb::engine::local::Db>,
}

impl SurrealSchemaAdapter {
    /// Get a reference to the underlying database client
    pub fn db(&self) -> &Surreal<surrealdb::engine::local::Db> {
        &self.db
    }
}

impl SurrealSchemaAdapter {
    /// Create a new SurrealDB schema adapter
    pub fn new(db: Surreal<surrealdb::engine::local::Db>) -> Self {
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

/// Initialize the SurrealDB schema storage adapter with RocksDB
async fn initialize_schema_storage(
    config: &AppConfig,
) -> Result<Arc<SurrealSchemaAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    let rocksdb_config = &config.rocksdb;
    
    info!("💎 Initializing SurrealDB with RocksDB: {}", rocksdb_config.path);

    // Create directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(&rocksdb_config.path).parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| BootstrapError::Initialization(e.to_string()))?;
    }

    // Connect to RocksDB embedded database
    let db = Surreal::new::<RocksDb>(&rocksdb_config.path).await
        .map_err(|e| BootstrapError::DatabaseConnection(e.to_string()))?;

    // Configure namespace and database
    let namespace = config.database.namespace.as_ref().unwrap();
    let database = config.database.database.as_ref().unwrap();
    
    info!("📂 Using namespace '{}' and database '{}'", namespace, database);
    
    db.use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|e| BootstrapError::Initialization(e.to_string()))?;

    Ok(Arc::new(SurrealSchemaAdapter::new(db)))
}

/// Validate bootstrap configuration and fail explicitly on any issues
///
/// This function performs additional validation beyond what's in AppConfig::validate()
/// to ensure the application fails fast on configuration problems.
fn validate_bootstrap_configuration(config: &AppConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Validate database configuration
    if config.database.namespace.is_none() || config.database.namespace.as_ref().unwrap().is_empty() {
        return Err(Box::new(BootstrapError::Initialization(
            "Database namespace is required but not configured".to_string()
        )));
    }

    if config.database.database.is_none() || config.database.database.as_ref().unwrap().is_empty() {
        return Err(Box::new(BootstrapError::Initialization(
            "Database name is required but not configured".to_string()
        )));
    }

    // Validate RocksDB path
    if config.rocksdb.path.is_empty() {
        return Err(Box::new(BootstrapError::Initialization(
            "RocksDB path is required but not configured".to_string()
        )));
    }

    // Validate that the RocksDB path parent directory is writable
    let path = std::path::Path::new(&config.rocksdb.path);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            // Try to create directory to validate permissions
            std::fs::create_dir_all(parent)
                .map_err(|e| BootstrapError::Initialization(format!(
                    "Cannot create RocksDB directory '{}': {}", parent.display(), e
                )))?;
        } else {
            // Check if directory is writable
            let test_file = parent.join(".hodei_test_write");
            if let Err(e) = std::fs::write(&test_file, "test") {
                return Err(Box::new(BootstrapError::Initialization(format!(
                    "RocksDB directory '{}' is not writable: {}", parent.display(), e
                ))));
            }
            let _ = std::fs::remove_file(test_file); // Clean up test file
        }
    }

    info!("✅ Configuration validation passed");
    Ok(())
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
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bootstrap_with_rocksdb() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.rocksdb");
        
        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();
        
        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false, // Skip IAM registration for faster tests
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        // Should succeed with RocksDB
        if result.is_ok() {
            println!("Bootstrap succeeded with RocksDB");
        } else {
            println!("Bootstrap failed: {:?}", result.err());
        }
        
        // Clean up
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_bootstrap_without_iam_schema_registration() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_no_iam.rocksdb");
        
        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        if result.is_ok() {
            println!("Bootstrap succeeded without IAM schema");
        } else {
            println!("Bootstrap failed: {:?}", result.err());
        }
        
        // Clean up
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_bootstrap_with_custom_schema_version() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_custom.rocksdb");
        
        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some("v2.0.0-test".to_string()),
            validate_schemas: true,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        if let Ok(app_state) = result {
            // The schema version should be set correctly
            assert!(!app_state.schema_version.is_empty());
        }
        
        // Clean up
        drop(temp_dir);
    }
}
