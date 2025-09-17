//! Dependency injection configuration for delete_policy feature
//!
//! This module provides the dependency injection container for the delete_policy use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyDeletionAuditor,
    SurrealPolicyDeletionRetriever,
    SurrealPolicyDeletionStorage,
    SurrealPolicyDeletionValidator,
};
use super::event_handler::SimplePolicyDeletionEventHandler;
use super::use_case::DeletePolicyUseCase;

/// Dependency injection container for delete_policy feature
pub struct DeletePolicyDIContainer {
    use_case: DeletePolicyUseCase<
        SurrealPolicyDeletionValidator,
        SurrealPolicyDeletionRetriever,
        SurrealPolicyDeletionStorage,
        SimplePolicyDeletionAuditor,
    >,
    event_handler: SimplePolicyDeletionEventHandler,
}

impl DeletePolicyDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        let event_handler = SimplePolicyDeletionEventHandler::new();

        Self {
            use_case,
            event_handler,
        }
    }

    /// Get the delete policy use case
    pub fn use_case(&self) -> &DeletePolicyUseCase<
        SurrealPolicyDeletionValidator,
        SurrealPolicyDeletionRetriever,
        SurrealPolicyDeletionStorage,
        SimplePolicyDeletionAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyDeletionEventHandler {
        &self.event_handler
    }
}

