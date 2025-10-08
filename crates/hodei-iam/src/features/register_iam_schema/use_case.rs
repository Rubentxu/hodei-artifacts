//! Use case for registering the IAM schema
//!
//! This use case orchestrates the registration of all IAM entity types and action types
//! with the policies engine, and triggers the schema building process.

use crate::features::register_iam_schema::dto::{
    RegisterIamSchemaCommand, RegisterIamSchemaResult,
};
use crate::features::register_iam_schema::error::RegisterIamSchemaError;
use crate::features::register_iam_schema::ports::RegisterIamSchemaPort;
use crate::internal::domain::actions::{
    AddUserToGroupAction, CreateGroupAction, CreateUserAction, DeleteArtifactAction,
    DeleteGroupAction, DeleteUserAction, DownloadArtifactAction, ListArtifactsAction,
    RemoveUserFromGroupAction, ShareArtifactAction, UpdateArtifactAction, UploadArtifactAction,
    ViewArtifactAction,
};
use crate::internal::domain::artifact::Artifact;
use crate::internal::domain::group::Group;
use crate::internal::domain::user::User;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn};

/// Use case for registering the IAM schema
///
/// This use case registers all IAM entity types (User, Group, Artifact) and action types
/// (CreateUser, DeleteUser, UploadArtifact, etc.) with the policies engine, then triggers
/// schema building and persistence.
///
/// # Architecture
///
/// This is an orchestration use case that coordinates multiple operations:
/// 1. Entity type registration via RegisterEntityTypeUseCase
/// 2. Action type registration via RegisterActionTypeUseCase
/// 3. Schema building via BuildSchemaUseCase (trait object for flexibility)
///
/// All dependencies are injected via use cases (not ports), enabling full testability
/// and compliance with the Dependency Inversion Principle.
pub struct RegisterIamSchemaUseCase {
    /// Use case for registering entity types
    entity_type_registrar: Arc<hodei_policies::register_entity_type::RegisterEntityTypeUseCase>,

    /// Use case for registering action types
    action_type_registrar: Arc<hodei_policies::register_action_type::RegisterActionTypeUseCase>,

    /// Schema builder port (trait object for flexibility)
    schema_builder: Arc<dyn SchemaBuilderPort>,
}

/// Internal port for schema building abstraction
///
/// This trait allows us to inject different schema building implementations
/// without exposing the generic parameter in the use case struct.
#[async_trait]
trait SchemaBuilderPort: Send + Sync {
    async fn build_and_persist(
        &self,
        version: Option<String>,
        validate: bool,
    ) -> Result<(String, String, bool), RegisterIamSchemaError>;
}

/// Adapter that wraps BuildSchemaUseCase to implement SchemaBuilderPort
struct BuildSchemaAdapter<S: hodei_policies::build_schema::ports::SchemaStoragePort> {
    use_case: hodei_policies::build_schema::BuildSchemaUseCase<S>,
}

#[async_trait]
impl<S: hodei_policies::build_schema::ports::SchemaStoragePort + 'static> SchemaBuilderPort
    for BuildSchemaAdapter<S>
{
    async fn build_and_persist(
        &self,
        version: Option<String>,
        validate: bool,
    ) -> Result<(String, String, bool), RegisterIamSchemaError> {
        let build_command =
            hodei_policies::build_schema::dto::BuildSchemaCommand { version, validate };

        let result = self.use_case.execute(build_command).await.map_err(|e| {
            warn!("Schema building failed: {}", e);
            RegisterIamSchemaError::SchemaBuildError(format!("Failed to build IAM schema: {}", e))
        })?;

        Ok((
            result.version.unwrap_or_else(|| "latest".to_string()),
            result.schema_id,
            result.validated,
        ))
    }
}

impl RegisterIamSchemaUseCase {
    /// Create a new IAM schema registration use case
    ///
    /// # Arguments
    ///
    /// * `entity_type_registrar` - Use case for registering entity types
    /// * `action_type_registrar` - Use case for registering action types
    /// * `schema_builder` - Use case for building and persisting schemas (generic)
    pub fn new<S: hodei_policies::build_schema::ports::SchemaStoragePort + 'static>(
        entity_type_registrar: Arc<hodei_policies::register_entity_type::RegisterEntityTypeUseCase>,
        action_type_registrar: Arc<hodei_policies::register_action_type::RegisterActionTypeUseCase>,
        schema_builder: hodei_policies::build_schema::BuildSchemaUseCase<S>,
    ) -> Self {
        Self {
            entity_type_registrar,
            action_type_registrar,
            schema_builder: Arc::new(BuildSchemaAdapter {
                use_case: schema_builder,
            }),
        }
    }

    /// Execute the IAM schema registration process
    ///
    /// This method performs the complete registration workflow:
    /// 1. Registers all IAM entity types
    /// 2. Registers all IAM action types
    /// 3. Builds and persists the schema
    ///
    /// # Arguments
    ///
    /// * `command` - The registration command with optional version and validation settings
    ///
    /// # Returns
    ///
    /// A registration result containing the schema version and statistics
    ///
    /// # Errors
    ///
    /// Returns an error if any step of the registration process fails:
    /// - Entity type registration failure
    /// - Action type registration failure
    /// - Schema building failure
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = RegisterIamSchemaCommand::new().with_validation(true);
    /// let result = use_case.execute(command).await?;
    /// println!("Registered IAM schema version: {}", result.schema_version);
    /// ```
    #[tracing::instrument(skip(self, command), fields(
        version = ?command.version,
        validate = command.validate
    ))]
    pub async fn execute(
        &self,
        command: RegisterIamSchemaCommand,
    ) -> Result<RegisterIamSchemaResult, RegisterIamSchemaError> {
        info!("Starting IAM schema registration");

        // Step 1: Register all IAM entity types
        let entity_count = self.register_entity_types()?;
        info!(
            entity_count = entity_count,
            "Successfully registered IAM entity types"
        );

        // Step 2: Register all IAM action types
        let action_count = self.register_action_types()?;
        info!(
            action_count = action_count,
            "Successfully registered IAM action types"
        );

        // Step 3: Build and persist the schema
        let (schema_version, schema_id, validated) = self
            .schema_builder
            .build_and_persist(command.version.clone(), command.validate)
            .await?;

        info!(
            schema_version = %schema_version,
            schema_id = %schema_id,
            validated = validated,
            "Successfully built and persisted IAM schema"
        );

        // Step 4: Return the registration result
        let result = RegisterIamSchemaResult::new(
            entity_count,
            action_count,
            schema_version,
            schema_id,
            validated,
        );

        info!(
            entity_types = result.entity_types_registered,
            action_types = result.action_types_registered,
            schema_version = %result.schema_version,
            "IAM schema registration completed successfully"
        );

        Ok(result)
    }

    /// Register all IAM entity types
    ///
    /// This method registers:
    /// - User
    /// - Group
    /// - Artifact
    ///
    /// # Returns
    ///
    /// The number of entity types successfully registered
    ///
    /// # Errors
    ///
    /// Returns an error if any entity type registration fails
    fn register_entity_types(&self) -> Result<usize, RegisterIamSchemaError> {
        let mut count = 0;

        // Register User entity type
        self.entity_type_registrar.register::<User>().map_err(|e| {
            RegisterIamSchemaError::EntityTypeRegistrationError(format!(
                "Failed to register User entity type: {}",
                e
            ))
        })?;
        count += 1;

        // Register Group entity type
        self.entity_type_registrar
            .register::<Group>()
            .map_err(|e| {
                RegisterIamSchemaError::EntityTypeRegistrationError(format!(
                    "Failed to register Group entity type: {}",
                    e
                ))
            })?;
        count += 1;

        // Register Artifact entity type
        self.entity_type_registrar
            .register::<Artifact>()
            .map_err(|e| {
                RegisterIamSchemaError::EntityTypeRegistrationError(format!(
                    "Failed to register Artifact entity type: {}",
                    e
                ))
            })?;
        count += 1;

        Ok(count)
    }

    /// Register all IAM action types
    ///
    /// This method registers:
    /// - CreateUser
    /// - DeleteUser
    /// - CreateGroup
    /// - DeleteGroup
    /// - AddUserToGroup
    /// - RemoveUserFromGroup
    /// - UploadArtifact
    /// - DownloadArtifact
    /// - ViewArtifact
    /// - UpdateArtifact
    /// - DeleteArtifact
    /// - ListArtifacts
    /// - ShareArtifact
    ///
    /// # Returns
    ///
    /// The number of action types successfully registered
    ///
    /// # Errors
    ///
    /// Returns an error if any action type registration fails
    fn register_action_types(&self) -> Result<usize, RegisterIamSchemaError> {
        let mut count = 0;

        // Register CreateUser action
        self.action_type_registrar
            .register::<CreateUserAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register CreateUser action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DeleteUser action
        self.action_type_registrar
            .register::<DeleteUserAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DeleteUser action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register CreateGroup action
        self.action_type_registrar
            .register::<CreateGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register CreateGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DeleteGroup action
        self.action_type_registrar
            .register::<DeleteGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DeleteGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register AddUserToGroup action
        self.action_type_registrar
            .register::<AddUserToGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register AddUserToGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register RemoveUserFromGroup action
        self.action_type_registrar
            .register::<RemoveUserFromGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register RemoveUserFromGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register UploadArtifact action
        self.action_type_registrar
            .register::<UploadArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register UploadArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DownloadArtifact action
        self.action_type_registrar
            .register::<DownloadArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DownloadArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register ViewArtifact action
        self.action_type_registrar
            .register::<ViewArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register ViewArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register UpdateArtifact action
        self.action_type_registrar
            .register::<UpdateArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register UpdateArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DeleteArtifact action
        self.action_type_registrar
            .register::<DeleteArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DeleteArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register ListArtifacts action
        self.action_type_registrar
            .register::<ListArtifactsAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register ListArtifacts action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register ShareArtifact action
        self.action_type_registrar
            .register::<ShareArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register ShareArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        Ok(count)
    }
}

/// Implementation of the RegisterIamSchemaPort trait for RegisterIamSchemaUseCase
///
/// This allows the use case to be used via the port abstraction,
/// enabling dependency inversion for other bounded contexts.
#[async_trait]
impl RegisterIamSchemaPort for RegisterIamSchemaUseCase {
    async fn register(
        &self,
        command: RegisterIamSchemaCommand,
    ) -> Result<RegisterIamSchemaResult, RegisterIamSchemaError> {
        self.execute(command).await
    }
}
