//! Dependency Injection for the register_entity_type feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::use_case::RegisterEntityTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

/// Factory for creating RegisterEntityTypeUseCase instances
pub struct RegisterEntityTypeUseCaseFactory;

impl RegisterEntityTypeUseCaseFactory {
    /// Creates a new RegisterEntityTypeUseCase instance
    ///
    /// This factory accepts a shared reference to an EngineBuilder,
    /// which is used to accumulate entity type registrations.
    ///
    /// # Arguments
    ///
    /// * `builder` - Shared reference to the EngineBuilder that will collect
    ///   entity type registrations
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::{Arc, Mutex};
    /// use hodei_policies::internal::engine::builder::EngineBuilder;
    /// use hodei_policies::features::register_entity_type::di::RegisterEntityTypeUseCaseFactory;
    ///
    /// let builder = Arc::new(Mutex::new(EngineBuilder::new()));
    /// let use_case = RegisterEntityTypeUseCaseFactory::build(builder);
    /// use_case.register::<MyEntityType>()?;
    /// ```
    pub fn build(builder: Arc<Mutex<EngineBuilder>>) -> RegisterEntityTypeUseCase {
        RegisterEntityTypeUseCase::new(builder)
    }
}
