//! Dependency injection configuration for create_policy feature
//!
//! This module provides the dependency injection container for the create_policy use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyCreationAuditor,
    SurrealPolicyCreationStorage,
    SurrealPolicyCreationValidator,
    SurrealPolicyExistenceChecker,
};
use super::event_handler::SimplePolicyCreationEventHandler;
use super::use_case::CreatePolicyUseCase;

/// Dependency injection container for create_policy feature
pub struct CreatePolicyDIContainer {
    use_case: CreatePolicyUseCase<
        SurrealPolicyCreationValidator,
        SurrealPolicyExistenceChecker,
        SurrealPolicyCreationStorage,
        SimplePolicyCreationAuditor,
    >,
    event_handler: SimplePolicyCreationEventHandler,
}

impl CreatePolicyDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let validator = Arc::new(SurrealPolicyCreationValidator::new());
        let existence_checker = Arc::new(SurrealPolicyExistenceChecker::new(db.clone()));
        let storage = Arc::new(SurrealPolicyCreationStorage::new(db));
        let auditor = Arc::new(SimplePolicyCreationAuditor::new());

        let use_case = CreatePolicyUseCase::new(
            validator,
            existence_checker,
            storage,
            auditor,
        );

        let event_handler = SimplePolicyCreationEventHandler::new();

        Self {
            use_case,
            event_handler,
        }
    }

    /// Get the create policy use case
    pub fn use_case(&self) -> &CreatePolicyUseCase<
        SurrealPolicyCreationValidator,
        SurrealPolicyExistenceChecker,
        SurrealPolicyCreationStorage,
        SimplePolicyCreationAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyCreationEventHandler {
        &self.event_handler
    }
}

