//! Ports (trait definitions) for the register_action_type feature
//!
//! This module defines the public interfaces for the register action type use case.
//! Following ISP (Interface Segregation Principle), the trait is minimal
//! and focused on a single responsibility.

use crate::features::register_action_type::dto::RegisterActionTypeCommand;
use crate::features::register_action_type::error::RegisterActionTypeError;
use async_trait::async_trait;

/// Port trait for registering action types in the Cedar schema builder
///
/// This trait defines the contract for action type registration operations.
/// It represents the use case's public interface.
#[async_trait]
pub trait RegisterActionTypePort: Send + Sync {
    /// Downcast to Any for accessing concrete implementation methods
    ///
    /// This method is needed to access generic methods like `register<T>()`
    /// that cannot be expressed in the trait due to Rust's trait object limitations.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Register an action type in the schema builder
    ///
    /// This method adds an action type definition to the accumulating schema builder,
    /// which will be consumed when the schema is built.
    ///
    /// # Arguments
    ///
    /// * `command` - The action type registration command containing the type definition
    ///
    /// # Returns
    ///
    /// Success if the action type was registered, or an error if registration failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The action type name is invalid
    /// - The action type is already registered
    /// - The builder lock cannot be acquired
    /// - The type definition is malformed
    async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError>;
}
