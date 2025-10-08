//! Bootstrap module for Hodei Artifacts API
//!
//! This module handles the complete application initialization:
//! 1. Infrastructure setup (database connections, adapters)
//! 2. Shared components creation (EngineBuilder)
//! 3. Use case instantiation with dependency injection
//! 4. IAM schema registration
//! 5. AppState construction

use crate::app_state::AppState;
use crate::config::Config;
use async_trait::async_trait;
use hodei_iam::features::register_iam_schema::{
    RegisterIamSchemaCommand, RegisterIamSchemaUseCase,
};
use hodei_policies::features::build_schema::BuildSchemaUseCase;
use hodei_policies::features::build_schema::error::BuildSchemaError;
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase;
use hodei_policies::features::load_schema::LoadSchemaUseCase;
use hodei_policies::features::playground_evaluate::adapters::{
    ContextConverterAdapter, PolicyEvaluatorAdapter, PolicyValidatorAdapter, SchemaLoaderAdapter,
};
use hodei_policies::features::playground_evaluate::di::PlaygroundEvaluateUseCaseFactory;
use hodei_policies::features::playground_evaluate::use_case::PlaygroundEvaluateUseCase;
use hodei_policies::features::register_action_type::RegisterActionTypeUseCase;
use hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use std::sync::{Arc, Mutex};
use surrealdb::{Surreal, engine::local::Mem};
use tracing::{info, warn};

/// Configuration for the bootstrap process
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Whether to register the IAM schema on startup
    pub register_iam_schema: bool,
    /// Optional specific schema version to use
    pub schema_version: Option<String>,
    /// Whether to validate schemas after building
    pub validate_schemas: bool,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            register_iam_schema: true,
            schema_version: None,
            validate_schemas: true,
        }
    }
}

/// Bootstrap the application and return the configured AppState
///
/// This function performs the complete application initialization sequence:
/// 1. Creates infrastructure adapters (storage, etc.)
/// 2. Instantiates the shared EngineBuilder for schema construction
/// 3. Creates all use cases with proper dependency injection
/// 4. Optionally registers the IAM schema during startup
/// 5. Returns the fully configured AppState ready for Axum
///
/// # Arguments
///
/// * `config` - Bootstrap configuration options
///
/// # Returns
///
/// A configured AppState ready to be used by Axum handlers
///
/// # Errors
///
/// Returns an error if:
/// - Infrastructure initialization fails
/// - IAM schema registration fails (if enabled)
/// - Any use case construction fails
///
/// # Example
///
/// ```rust,ignore
/// let config = BootstrapConfig::default();
/// let app_state = bootstrap(config).await?;
/// let app = Router::new().with_state(app_state);
/// ```
pub async fn bootstrap(
    config: BootstrapConfig,
) -> Result<AppState<SurrealSchemaAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Hodei Artifacts API bootstrap");

    // Step 1: Initialize infrastructure
    info!("📦 Initializing infrastructure adapters");
    let schema_storage = initialize_schema_storage().await?;

    // Step 2: Create shared components
    info!("🔧 Creating shared components");
    let engine_builder = create_engine_builder();

    // Step 3: Instantiate use cases with dependency injection
    info!("🏗️  Instantiating use cases");
    let use_cases = create_use_cases(engine_builder.clone(), schema_storage.clone());

    // Step 4: Optionally register IAM schema
    let schema_version = if config.register_iam_schema {
        info!("📝 Registering IAM schema");
        let result = register_iam_schema(
            &use_cases.register_iam_schema,
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

    // Step 5: Create AppState
    info!("🎯 Creating application state");
    let app_state = AppState::new(
        schema_version.clone(),
        use_cases.register_iam_schema,
        use_cases.register_entity_type,
        use_cases.register_action_type,
        use_cases.build_schema,
        use_cases.load_schema,
        use_cases.validate_policy,
        use_cases.evaluate_policies,
        use_cases.playground_evaluate,
    );

    info!(
        "✅ Bootstrap completed successfully (schema version: {})",
        schema_version
    );

    Ok(app_state)
}

/// SurrealDB schema storage adapter for production
#[derive(Clone)]
pub struct SurrealSchemaAdapter {
    db: Arc<Surreal<surrealdb::engine::local::Db>>,
}

impl SurrealSchemaAdapter {
    pub fn new(db: Arc<Surreal<surrealdb::engine::local::Db>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SchemaStoragePort for SurrealSchemaAdapter {
    async fn save_schema(
        &self,
        schema: String,
        version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        let schema_id = version.unwrap_or_else(|| "latest".to_string());
        let schema_id_clone = schema_id.clone();
        let schema_clone = schema.clone();

        // Store schema in SurrealDB
        let _: Option<serde_json::Value> = self
            .db
            .query("CREATE schema SET id = $id, content = $content, created_at = time::now()")
            .bind(("id", schema_id_clone))
            .bind(("content", schema_clone))
            .await
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to save schema: {}", e))
            })?
            .take(0)
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to parse result: {}", e))
            })?;

        Ok(schema_id)
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        let result: Option<serde_json::Value> = self
            .db
            .query("SELECT content FROM schema ORDER BY created_at DESC LIMIT 1")
            .await
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to query schema: {}", e))
            })?
            .take(0)
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to parse result: {}", e))
            })?;

        if let Some(value) = result {
            if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                return Ok(Some(content.to_string()));
            }
        }

        Ok(None)
    }

    async fn get_schema_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        let version_clone = version.to_string();
        let result: Option<serde_json::Value> = self
            .db
            .query("SELECT content FROM schema WHERE id = $version")
            .bind(("version", version_clone))
            .await
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to query schema: {}", e))
            })?
            .take(0)
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to parse result: {}", e))
            })?;

        if let Some(value) = result {
            if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                return Ok(Some(content.to_string()));
            }
        }

        Ok(None)
    }

    async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError> {
        let schema_id_clone = schema_id.to_string();
        let result: Option<serde_json::Value> = self
            .db
            .query("DELETE schema WHERE id = $id")
            .bind(("id", schema_id_clone))
            .await
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to delete schema: {}", e))
            })?
            .take(0)
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to parse result: {}", e))
            })?;

        Ok(result.is_some())
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        let result: Vec<serde_json::Value> = self
            .db
            .query("SELECT id FROM schema ORDER BY created_at DESC")
            .await
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to list schemas: {}", e))
            })?
            .take(0)
            .map_err(|e| {
                BuildSchemaError::SchemaStorageError(format!("Failed to parse result: {}", e))
            })?;

        let versions: Vec<String> = result
            .into_iter()
            .filter_map(|value| {
                value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(versions)
    }
}

/// Initialize schema storage adapter
///
/// Uses SurrealDB embedded for persistence.
async fn initialize_schema_storage()
-> Result<Arc<SurrealSchemaAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    info!("Creating SurrealDB schema storage adapter");

    // Create embedded SurrealDB instance
    let db = Surreal::new::<Mem>(())
        .await
        .map_err(|e| format!("Failed to create SurrealDB instance: {}", e))?;

    // Use default namespace and database for embedded mode
    db.use_ns("hodei")
        .use_db("artifacts")
        .await
        .map_err(|e| format!("Failed to set namespace/database: {}", e))?;

    let adapter = Arc::new(SurrealSchemaAdapter::new(Arc::new(db)));

    info!("✅ SurrealDB schema storage adapter initialized");
    Ok(adapter)
}

/// Create the shared EngineBuilder for schema construction
///
/// The EngineBuilder accumulates entity and action type registrations
/// and is shared across all registration use cases.
fn create_engine_builder() -> Arc<Mutex<hodei_policies::EngineBuilder>> {
    info!("Creating shared EngineBuilder");
    Arc::new(Mutex::new(hodei_policies::EngineBuilder::new()))
}

/// Struct to hold all instantiated use cases
struct UseCases<S: hodei_policies::features::build_schema::ports::SchemaStoragePort> {
    register_iam_schema: Arc<RegisterIamSchemaUseCase>,
    register_entity_type: Arc<RegisterEntityTypeUseCase>,
    register_action_type: Arc<RegisterActionTypeUseCase>,
    build_schema: Arc<BuildSchemaUseCase<S>>,
    load_schema: Arc<LoadSchemaUseCase<S>>,
    validate_policy: Arc<ValidatePolicyUseCase<S>>,
    evaluate_policies: Arc<EvaluatePoliciesUseCase>,
    playground_evaluate: Arc<PlaygroundEvaluateUseCase>,
}

/// Create all use cases with proper dependency injection
///
/// This function implements the Composition Root pattern, instantiating
/// all use cases with their dependencies properly injected.
///
/// # Arguments
///
/// * `engine_builder` - Shared EngineBuilder for schema construction
/// * `schema_storage` - Schema storage adapter implementation
///
/// # Returns
///
/// A struct containing all instantiated use cases
fn create_use_cases<S>(
    engine_builder: Arc<Mutex<hodei_policies::EngineBuilder>>,
    schema_storage: Arc<S>,
) -> UseCases<S>
where
    S: hodei_policies::features::build_schema::ports::SchemaStoragePort + Clone + 'static,
{
    // 1. Create entity type registration use case
    info!("  - Creating RegisterEntityTypeUseCase");
    let register_entity_type = Arc::new(RegisterEntityTypeUseCase::new(engine_builder.clone()));

    // 2. Create action type registration use case
    info!("  - Creating RegisterActionTypeUseCase");
    let register_action_type = Arc::new(RegisterActionTypeUseCase::new(engine_builder.clone()));

    // 3. Create schema building use case (without Arc initially)
    info!("  - Creating BuildSchemaUseCase");
    let build_schema_uc = BuildSchemaUseCase::new(engine_builder.clone(), schema_storage.clone());

    // 4. Create IAM schema registration use case (consumes build_schema_uc)
    info!("  - Creating RegisterIamSchemaUseCase");
    let register_iam_schema = Arc::new(RegisterIamSchemaUseCase::new(
        register_entity_type.clone(),
        register_action_type.clone(),
        build_schema_uc,
    ));

    // 5. Create another BuildSchemaUseCase for direct use (needed by other parts)
    let build_schema = Arc::new(BuildSchemaUseCase::new(
        engine_builder.clone(),
        schema_storage.clone(),
    ));

    // 6. Create schema loading use case
    info!("  - Creating LoadSchemaUseCase");
    let load_schema = Arc::new(LoadSchemaUseCase::new(schema_storage.clone()));

    // 7. Create policy validation use case
    info!("  - Creating ValidatePolicyUseCase");
    let validate_policy = Arc::new(ValidatePolicyUseCase::with_schema_storage(
        schema_storage.clone(),
    ));

    // 8. Create policy evaluation use case
    info!("  - Creating EvaluatePoliciesUseCase");
    let evaluate_policies = Arc::new(EvaluatePoliciesUseCase::new(schema_storage.clone()));

    // 9. Create playground evaluate use case
    info!("  - Creating PlaygroundEvaluateUseCase");
    let playground_evaluate = create_playground_evaluate_use_case(schema_storage.clone());

    UseCases {
        register_iam_schema,
        register_entity_type,
        register_action_type,
        build_schema,
        load_schema,
        validate_policy,
        evaluate_policies,
        playground_evaluate,
    }
}

/// Create playground evaluate use case with all adapters
///
/// This function creates all the adapters needed for the playground evaluate
/// feature and builds the use case using the factory.
///
/// # Arguments
///
/// * `schema_storage` - Schema storage adapter for loading schemas
///
/// # Returns
///
/// A configured PlaygroundEvaluateUseCase ready for use
fn create_playground_evaluate_use_case<S>(schema_storage: Arc<S>) -> Arc<PlaygroundEvaluateUseCase>
where
    S: hodei_policies::features::build_schema::ports::SchemaStoragePort + Clone + 'static,
{
    // Create all adapters
    let schema_loader = Arc::new(SchemaLoaderAdapter::new(schema_storage));
    let policy_validator = Arc::new(PolicyValidatorAdapter::new());
    let policy_evaluator = Arc::new(PolicyEvaluatorAdapter::new());
    let context_converter = Arc::new(ContextConverterAdapter::new());

    // Build use case using factory
    let use_case = PlaygroundEvaluateUseCaseFactory::build(
        schema_loader,
        policy_validator,
        policy_evaluator,
        context_converter,
    );

    Arc::new(use_case)
}

/// Register the IAM schema during bootstrap
///
/// This function executes the IAM schema registration use case,
/// which registers all IAM entity types (User, Group) and action types
/// (CreateUser, DeleteUser, etc.) with the policies engine.
///
/// # Arguments
///
/// * `use_case` - The RegisterIamSchemaUseCase instance
/// * `version` - Optional specific schema version
/// * `validate` - Whether to validate the schema after building
///
/// # Returns
///
/// The registration result containing schema version and statistics
///
/// # Errors
///
/// Returns an error if schema registration fails
async fn register_iam_schema(
    use_case: &RegisterIamSchemaUseCase,
    version: Option<String>,
    validate: bool,
) -> Result<
    hodei_iam::features::register_iam_schema::RegisterIamSchemaResult,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let mut command = RegisterIamSchemaCommand::new().with_validation(validate);

    if let Some(v) = version {
        command = command.with_version(v);
    }

    let result = use_case.execute(command).await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bootstrap_with_default_config() {
        let config = BootstrapConfig::default();
        let result = bootstrap(config).await;

        assert!(
            result.is_ok(),
            "Bootstrap should succeed with default config"
        );

        let app_state = result.unwrap();
        assert!(!app_state.schema_version.is_empty());
    }

    #[tokio::test]
    async fn test_bootstrap_without_iam_schema_registration() {
        let config = BootstrapConfig {
            register_iam_schema: false,
            schema_version: Some("test-version".to_string()),
            validate_schemas: false,
        };

        let result = bootstrap(config).await;

        assert!(
            result.is_ok(),
            "Bootstrap should succeed without IAM registration"
        );

        let app_state = result.unwrap();
        assert_eq!(app_state.schema_version, "test-version");
    }

    #[tokio::test]
    async fn test_bootstrap_with_custom_schema_version() {
        let config = BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some("v1.2.3".to_string()),
            validate_schemas: true,
        };

        let result = bootstrap(config).await;

        assert!(
            result.is_ok(),
            "Bootstrap should succeed with custom version"
        );

        let app_state = result.unwrap();
        // Schema version will be set by the registration result, not the config
        assert!(!app_state.schema_version.is_empty());
    }
}
