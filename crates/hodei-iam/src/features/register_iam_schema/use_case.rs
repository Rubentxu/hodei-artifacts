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
use hodei_policies::build_schema::dto::BuildSchemaCommand;
use hodei_policies::build_schema::ports::BuildSchemaPort;
use hodei_policies::register_action_type::RegisterActionTypeUseCase;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::RegisterEntityTypeUseCase;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
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
/// This is an orchestration use case that coordinates multiple operations via ports:
/// 1. Entity type registration via RegisterEntityTypePort
/// 2. Action type registration via RegisterActionTypePort
/// 3. Schema building via BuildSchemaPort
///
/// All dependencies are injected via ports (traits), enabling full testability
/// and compliance with the Dependency Inversion Principle.
pub struct RegisterIamSchemaUseCase {
    /// Port for registering entity types
    entity_type_registrar: Arc<dyn RegisterEntityTypePort>,

    /// Port for registering action types
    action_type_registrar: Arc<dyn RegisterActionTypePort>,

    /// Port for building and persisting schemas
    schema_builder: Arc<dyn BuildSchemaPort>,
}

impl RegisterIamSchemaUseCase {
    /// Create a new IAM schema registration use case
    ///
    /// # Arguments
    ///
    /// * `entity_type_registrar` - Port for registering entity types
    /// * `action_type_registrar` - Port for registering action types
    /// * `schema_builder` - Port for building and persisting schemas
    pub fn new(
        entity_type_registrar: Arc<dyn RegisterEntityTypePort>,
        action_type_registrar: Arc<dyn RegisterActionTypePort>,
        schema_builder: Arc<dyn BuildSchemaPort>,
    ) -> Self {
        Self {
            entity_type_registrar,
            action_type_registrar,
            schema_builder,
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
        let entity_count = self.register_entity_types().await?;
        info!(
            entity_count = entity_count,
            "Successfully registered IAM entity types"
        );

        // Step 2: Register all IAM action types
        let action_count = self.register_action_types().await?;
        info!(
            action_count = action_count,
            "Successfully registered IAM action types"
        );

        // Step 3: Build and persist the schema
        let build_command = BuildSchemaCommand {
            version: command.version.clone(),
            validate: command.validate,
        };

        let build_result = self
            .schema_builder
            .execute(build_command)
            .await
            .map_err(|e| {
                warn!("Schema building failed: {}", e);
                RegisterIamSchemaError::SchemaBuildError(format!(
                    "Failed to build IAM schema: {}",
                    e
                ))
            })?;

        let schema_version = build_result.version.unwrap_or_else(|| "latest".to_string());
        let schema_id = build_result.schema_id;
        let validated = build_result.validated;

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
    /// Note: We need to downcast to the concrete use case to call the generic register method.
    /// This is a limitation of the current design where the port trait doesn't support
    /// generic registration.
    ///
    /// # Returns
    ///
    /// The number of entity types successfully registered
    ///
    /// # Errors
    ///
    /// Returns an error if any entity type registration fails
    async fn register_entity_types(&self) -> Result<usize, RegisterIamSchemaError> {
        let mut count = 0;

        // We need to downcast to access the generic register method
        // This is safe because we control the factory and know the concrete type
        let concrete_uc = self
            .entity_type_registrar
            .as_any()
            .downcast_ref::<RegisterEntityTypeUseCase>()
            .ok_or_else(|| {
                RegisterIamSchemaError::EntityTypeRegistrationError(
                    "Failed to downcast entity type registrar".to_string(),
                )
            })?;

        // Register User entity type
        concrete_uc.register::<User>().map_err(|e| {
            RegisterIamSchemaError::EntityTypeRegistrationError(format!(
                "Failed to register User entity type: {}",
                e
            ))
        })?;
        count += 1;

        // Register Group entity type
        concrete_uc.register::<Group>().map_err(|e| {
            RegisterIamSchemaError::EntityTypeRegistrationError(format!(
                "Failed to register Group entity type: {}",
                e
            ))
        })?;
        count += 1;

        // Register Artifact entity type
        concrete_uc.register::<Artifact>().map_err(|e| {
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
    /// Note: We need to downcast to the concrete use case to call the generic register method.
    ///
    /// # Returns
    ///
    /// The number of action types successfully registered
    ///
    /// # Errors
    ///
    /// Returns an error if any action type registration fails
    async fn register_action_types(&self) -> Result<usize, RegisterIamSchemaError> {
        let mut count = 0;

        // We need to downcast to access the generic register method
        let concrete_uc = self
            .action_type_registrar
            .as_any()
            .downcast_ref::<RegisterActionTypeUseCase>()
            .ok_or_else(|| {
                RegisterIamSchemaError::ActionTypeRegistrationError(
                    "Failed to downcast action type registrar".to_string(),
                )
            })?;

        // Register CreateUser action
        concrete_uc.register::<CreateUserAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register CreateUser action: {}",
                e
            ))
        })?;
        count += 1;

        // Register DeleteUser action
        concrete_uc.register::<DeleteUserAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register DeleteUser action: {}",
                e
            ))
        })?;
        count += 1;

        // Register CreateGroup action
        concrete_uc.register::<CreateGroupAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register CreateGroup action: {}",
                e
            ))
        })?;
        count += 1;

        // Register DeleteGroup action
        concrete_uc.register::<DeleteGroupAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register DeleteGroup action: {}",
                e
            ))
        })?;
        count += 1;

        // Register AddUserToGroup action
        concrete_uc
            .register::<AddUserToGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register AddUserToGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register RemoveUserFromGroup action
        concrete_uc
            .register::<RemoveUserFromGroupAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register RemoveUserFromGroup action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register UploadArtifact action
        concrete_uc
            .register::<UploadArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register UploadArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DownloadArtifact action
        concrete_uc
            .register::<DownloadArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DownloadArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register ViewArtifact action
        concrete_uc.register::<ViewArtifactAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register ViewArtifact action: {}",
                e
            ))
        })?;
        count += 1;

        // Register UpdateArtifact action
        concrete_uc
            .register::<UpdateArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register UpdateArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register DeleteArtifact action
        concrete_uc
            .register::<DeleteArtifactAction>()
            .map_err(|e| {
                RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                    "Failed to register DeleteArtifact action: {}",
                    e
                ))
            })?;
        count += 1;

        // Register ListArtifacts action
        concrete_uc.register::<ListArtifactsAction>().map_err(|e| {
            RegisterIamSchemaError::ActionTypeRegistrationError(format!(
                "Failed to register ListArtifacts action: {}",
                e
            ))
        })?;
        count += 1;

        // Register ShareArtifact action
        concrete_uc.register::<ShareArtifactAction>().map_err(|e| {
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
