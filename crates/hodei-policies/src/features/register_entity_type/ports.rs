//! Ports (trait definitions) for the register_entity_type feature
//!
//! This module defines the public interfaces for the register entity type use case.
//! Following ISP (Interface Segregation Principle), the trait is minimal
//! and focused on a single responsibility.

use crate::features::register_entity_type::dto::RegisterEntityTypeCommand;
use crate::features::register_entity_type::error::RegisterEntityTypeError;
use async_trait::async_trait;

/// Port trait for registering entity types in the Cedar schema builder
///
/// This trait defines the contract for entity type registration operations.
/// It represents the use case's public interface.
#[async_trait]
pub trait RegisterEntityTypePort: Send + Sync {
    /// Downcast to Any for accessing concrete implementation methods
    ///
    /// This method is needed to access generic methods like `register<T>()`
    /// that cannot be expressed in the trait due to Rust's trait object limitations.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Register an entity type in the schema builder
    ///
    /// This method adds an entity type definition to the accumulating schema builder,
    /// which will be consumed when the schema is built.
    ///
    /// # Arguments
    ///
    /// * `command` - The entity type registration command containing the type definition
    ///
    /// # Returns
    ///
    /// Success if the entity type was registered, or an error if registration failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entity type name is invalid
    /// - The entity type is already registered
    /// - The builder lock cannot be acquired
    /// - The type definition is malformed
    async fn execute(
        &self,
        command: RegisterEntityTypeCommand,
    ) -> Result<(), RegisterEntityTypeError>;
}
