use crate::features::register_action_type::dto::RegisterActionTypeCommand;
use crate::features::register_action_type::error::RegisterActionTypeError;
use crate::features::register_action_type::ports::RegisterActionTypePort;
use crate::internal::engine::builder::{EngineBuilder, action_type_dsl};
use async_trait::async_trait;
use cedar_policy::SchemaFragment;
use kernel::ActionTrait;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Use case for registering action types in the Cedar schema
///
/// This use case manages the registration of action types that will be used
/// in policy evaluation. Each action type must be registered before policies
/// referencing it can be validated or evaluated.
///
/// # Architecture
///
/// This is a synchronous use case that manipulates the internal EngineBuilder.
/// The builder is shared across registration operations and is later consumed
/// by the `build_schema` feature.
pub struct RegisterActionTypeUseCase {
    /// Internal schema builder for action type registration
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterActionTypeUseCase {
    /// Create a new action type registration use case
    ///
    /// # Arguments
    ///
    /// * `builder` - Shared reference to the EngineBuilder
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    /// Register an action type for schema generation
    ///
    /// This method is generic over types that implement `ActionTrait`,
    /// allowing type-safe registration without runtime indirection.
    ///
    /// # Type Parameters
    ///
    /// * `A` - The action type to register, must implement `ActionTrait`
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if registration succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema generation fails for the action type
    /// - The action type is invalid
    /// - An internal error occurs during registration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::register_action_type::RegisterActionTypeUseCase;
    ///
    /// let use_case = RegisterActionTypeUseCase::new(builder);
    /// use_case.register::<CreateUserAction>()?;
    /// use_case.register::<DeleteGroupAction>()?;
    /// ```
    #[tracing::instrument(skip(self), fields(
        action_name = A::name(),
        service = A::service_name().as_str(),
        applies_to_principal = A::applies_to_principal(),
        applies_to_resource = A::applies_to_resource()
    ))]
    pub fn register<A: ActionTrait>(&self) -> Result<(), RegisterActionTypeError> {
        info!(
            "Registering action type: {} (service: {}, principal: {}, resource: {})",
            A::name(),
            A::service_name().as_str(),
            A::applies_to_principal(),
            A::applies_to_resource()
        );

        self.register_command(&RegisterActionTypeCommand::from_type::<A>())
    }

    /// Register the action type described by a command
    ///
    /// The command is turned into the Cedar schema DSL, parsed into a schema
    /// fragment and handed over to the builder. The registration is idempotent
    /// per action name: registering the same action twice keeps one fragment.
    fn register_command(
        &self,
        command: &RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError> {
        if command.action_name.trim().is_empty() {
            return Err(RegisterActionTypeError::InvalidActionType(
                "the action name must not be empty".to_string(),
            ));
        }
        if command.applies_to_principal.trim().is_empty() {
            return Err(RegisterActionTypeError::InvalidActionType(format!(
                "action '{}' declares no principal type",
                command.action_name
            )));
        }
        if command.applies_to_resource.trim().is_empty() {
            return Err(RegisterActionTypeError::InvalidActionType(format!(
                "action '{}' declares no resource type",
                command.action_name
            )));
        }

        let dsl = action_type_dsl(
            &command.action_name,
            &command.applies_to_principal,
            &command.applies_to_resource,
        );

        let (fragment, _warnings) = SchemaFragment::from_cedarschema_str(&dsl)
            .map_err(|e| RegisterActionTypeError::SchemaGenerationError(e.to_string()))?;

        let mut builder = self.builder.lock().map_err(|e| {
            RegisterActionTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        builder.register_action_fragment(command.action_name.clone(), fragment);

        info!(
            "Successfully registered action type: {} (total actions: {})",
            command.action_name,
            builder.action_count()
        );

        Ok(())
    }

    /// Get the number of registered action types
    ///
    /// This is useful for diagnostics and testing.
    pub fn count(&self) -> Result<usize, RegisterActionTypeError> {
        let builder = self.builder.lock().map_err(|e| {
            RegisterActionTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        Ok(builder.action_count())
    }

    /// Clear all registered action types
    ///
    /// This removes all registered action types from the builder.
    /// Useful for testing or when you need to start fresh.
    pub fn clear(&self) -> Result<(), RegisterActionTypeError> {
        let mut builder = self.builder.lock().map_err(|e| {
            RegisterActionTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        builder.clear();
        info!("Cleared all registered action types");

        Ok(())
    }

    /// Downcast helper for accessing the concrete use case
    pub fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Execute action type registration from a command
    ///
    /// This is the command-based interface that satisfies the port trait. It
    /// registers the action type the command describes; it is not a stub.
    pub async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError> {
        self.register_command(&command)
    }
}

/// Implementation of RegisterActionTypePort trait for RegisterActionTypeUseCase
#[async_trait]
impl RegisterActionTypePort for RegisterActionTypeUseCase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError> {
        self.execute(command).await
    }
}
