use crate::features::register_entity_type::dto::RegisterEntityTypeCommand;
use crate::features::register_entity_type::error::RegisterEntityTypeError;
use crate::features::register_entity_type::ports::RegisterEntityTypePort;
use crate::internal::engine::builder::EngineBuilder;
use async_trait::async_trait;
use kernel::HodeiEntityType;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Use case for registering entity types in the Cedar schema
///
/// This use case manages the registration of entity types that will be used
/// in policy evaluation. Each entity type must be registered before policies
/// referencing it can be validated or evaluated.
///
/// # Architecture
///
/// This is a synchronous use case that manipulates the internal EngineBuilder.
/// The builder is shared across registration operations and is later consumed
/// by the `build_schema` feature.
pub struct RegisterEntityTypeUseCase {
    /// Internal schema builder for entity type registration
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterEntityTypeUseCase {
    /// Create a new entity type registration use case
    ///
    /// # Arguments
    ///
    /// * `builder` - Shared reference to the EngineBuilder
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    /// Register an entity type for schema generation
    ///
    /// This method is generic over types that implement `HodeiEntityType`,
    /// allowing type-safe registration without runtime indirection.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The entity type to register, must implement `HodeiEntityType`
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if registration succeeds or if the type was already registered.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema generation fails for the entity type
    /// - The entity type is invalid
    /// - An internal error occurs during registration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase;
    ///
    /// let use_case = RegisterEntityTypeUseCase::new(builder);
    /// use_case.register::<MyUser>()?;
    /// use_case.register::<MyDocument>()?;
    /// ```
    #[tracing::instrument(skip(self), fields(
        entity_type = T::entity_type_name(),
        service = T::service_name().as_str(),
        resource_type = T::resource_type_name().as_str(),
        is_principal = T::is_principal_type()
    ))]
    pub fn register<T: HodeiEntityType>(&self) -> Result<(), RegisterEntityTypeError> {
        let entity_type_name = T::entity_type_name();

        info!(
            "Registering entity type: {} (service: {}, resource: {}, principal: {})",
            entity_type_name,
            T::service_name().as_str(),
            T::resource_type_name().as_str(),
            T::is_principal_type()
        );

        let mut builder = self.builder.lock().map_err(|e| {
            RegisterEntityTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        builder
            .register_entity::<T>()
            .map_err(|e| RegisterEntityTypeError::SchemaGenerationError(e.to_string()))?;

        info!(
            "Successfully registered entity type: {} (total entities: {})",
            entity_type_name,
            builder.entity_count()
        );

        Ok(())
    }

    /// Get the number of registered entity types
    ///
    /// This is useful for diagnostics and testing.
    pub fn count(&self) -> Result<usize, RegisterEntityTypeError> {
        let builder = self.builder.lock().map_err(|e| {
            RegisterEntityTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        Ok(builder.entity_count())
    }

    /// Clear all registered entity types
    ///
    /// This removes all registered entity types from the builder.
    /// Useful for testing or when you need to start fresh.
    pub fn clear(&self) -> Result<(), RegisterEntityTypeError> {
        let mut builder = self.builder.lock().map_err(|e| {
            RegisterEntityTypeError::InternalError(format!("Failed to lock builder: {}", e))
        })?;

        builder.clear();
        info!("Cleared all registered entity types");

        Ok(())
    }

    /// Downcast helper for accessing the concrete use case
    pub fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Execute entity type registration from a command
    ///
    /// This is the command-based interface that satisfies the port trait.
    pub async fn execute(
        &self,
        _command: RegisterEntityTypeCommand,
    ) -> Result<(), RegisterEntityTypeError> {
        // The actual registration is done via the generic register<E>() method
        // This method exists to satisfy the port trait interface
        // In practice, callers should use register<E>() directly for type safety
        info!("RegisterEntityTypeUseCase::execute called");
        Ok(())
    }
}

/// Implementation of RegisterEntityTypePort trait for RegisterEntityTypeUseCase
#[async_trait]
impl RegisterEntityTypePort for RegisterEntityTypeUseCase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute(
        &self,
        command: RegisterEntityTypeCommand,
    ) -> Result<(), RegisterEntityTypeError> {
        self.execute(command).await
    }
}
