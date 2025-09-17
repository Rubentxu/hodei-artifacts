//! Dependency injection configuration for get_policy feature
//!
//! This module provides the dependency injection container for the get_policy use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyRetrievalAuditor,
    SurrealPolicyAccessValidator,
    SurrealPolicyRetrievalStorage,
};
use super::event_handler::SimplePolicyRetrievalEventHandler;
use super::use_case::GetPolicyUseCase;

/// Dependency injection container for get_policy feature
pub struct GetPolicyDIContainer {
    use_case: GetPolicyUseCase<
        SurrealPolicyAccessValidator,
        SurrealPolicyRetrievalStorage,
        SimplePolicyRetrievalAuditor,
    >,
    event_handler: SimplePolicyRetrievalEventHandler,
}

impl GetPolicyDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        let event_handler = SimplePolicyRetrievalEventHandler::new();

        Self {
            use_case,
            event_handler,
        }
    }

    /// Get the get policy use case
    pub fn use_case(&self) -> &GetPolicyUseCase<
        SurrealPolicyAccessValidator,
        SurrealPolicyRetrievalStorage,
        SimplePolicyRetrievalAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyRetrievalEventHandler {
        &self.event_handler
    }
}

