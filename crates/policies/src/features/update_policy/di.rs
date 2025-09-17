//! Dependency injection configuration for update_policy feature
//!
//! This module provides the dependency injection container for the update_policy use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyUpdateAuditor,
    SurrealPolicyRetriever,
    SurrealPolicyUpdateStorage,
    SurrealPolicyUpdateValidator,
};
use super::event_handler::SimplePolicyUpdateEventHandler;
use super::use_case::UpdatePolicyUseCase;

/// Dependency injection container for update_policy feature
pub struct UpdatePolicyDIContainer {
    use_case: UpdatePolicyUseCase<
        SurrealPolicyUpdateValidator,
        SurrealPolicyRetriever,
        SurrealPolicyUpdateStorage,
        SimplePolicyUpdateAuditor,
    >,
    event_handler: SimplePolicyUpdateEventHandler,
}

impl UpdatePolicyDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let validator = Arc::new(SurrealPolicyUpdateValidator::new());
        let retriever = Arc::new(SurrealPolicyRetriever::new(db.clone()));
        let storage = Arc::new(SurrealPolicyUpdateStorage::new(db));
        let auditor = Arc::new(SimplePolicyUpdateAuditor::new());

        let use_case = UpdatePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        let event_handler = SimplePolicyUpdateEventHandler::new();

        Self {
            use_case,
            event_handler,
        }
    }

    /// Get the update policy use case
    pub fn use_case(&self) -> &UpdatePolicyUseCase<
        SurrealPolicyUpdateValidator,
        SurrealPolicyRetriever,
        SurrealPolicyUpdateStorage,
        SimplePolicyUpdateAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyUpdateEventHandler {
        &self.event_handler
    }
}

