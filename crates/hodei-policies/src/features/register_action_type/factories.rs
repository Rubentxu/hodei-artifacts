//! Factory functions for the register_action_type feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::register_action_type::ports::RegisterActionTypePort;
use crate::features::register_action_type::use_case::RegisterActionTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

/// Creates a RegisterActionTypeUseCase with the provided dependencies
///
/// This factory receives an already-constructed shared EngineBuilder
/// and assembles a use case for registering action types.
///
/// # Arguments
///
/// * `builder` - Pre-constructed shared reference to the EngineBuilder
///
/// # Returns
///
/// An `Arc<dyn RegisterActionTypePort>` trait object, enabling dependency inversion
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::register_action_type::factories;
/// use std::sync::{Arc, Mutex};
///
/// // Composition root creates the shared builder
/// let builder = Arc::new(Mutex::new(EngineBuilder::new()));
///
/// // Factory receives the builder and assembles the use case
/// let use_case = factories::create_register_action_type_use_case(builder);
/// use_case.register::<MyActionType>()?;
/// ```
pub fn create_register_action_type_use_case(
    builder: Arc<Mutex<EngineBuilder>>,
) -> Arc<dyn RegisterActionTypePort> {
    Arc::new(RegisterActionTypeUseCase::new(builder))
}
