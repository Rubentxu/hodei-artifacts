//! Dependency Injection for the register_action_type feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::use_case::RegisterActionTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

/// Factory for creating RegisterActionTypeUseCase instances
pub struct RegisterActionTypeUseCaseFactory;

impl RegisterActionTypeUseCaseFactory {
    /// Creates a new RegisterActionTypeUseCase instance
    ///
    /// This factory accepts a shared reference to an EngineBuilder,
    /// which is used to accumulate action type registrations.
    ///
    /// # Arguments
    ///
    /// * `builder` - Shared reference to the EngineBuilder that will collect
    ///   action type registrations
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::{Arc, Mutex};
    /// use hodei_policies::internal::engine::builder::EngineBuilder;
    /// use hodei_policies::features::register_action_type::di::RegisterActionTypeUseCaseFactory;
    ///
    /// let builder = Arc::new(Mutex::new(EngineBuilder::new()));
    /// let use_case = RegisterActionTypeUseCaseFactory::build(builder);
    /// use_case.register::<MyActionType>()?;
    /// ```
    pub fn build(builder: Arc<Mutex<EngineBuilder>>) -> RegisterActionTypeUseCase {
        RegisterActionTypeUseCase::new(builder)
    }
}
