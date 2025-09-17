//! Dependency injection configuration for list_policies feature
//!
//! This module provides the dependency injection container for the list_policies use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyListingAuditor,
    SurrealListQueryValidator,
    SurrealPolicyListingStorage,
};
use super::event_handler::SimplePolicyListingEventHandler;
use super::ports::ListPoliciesConfig;
use super::use_case::ListPoliciesUseCase;

/// Dependency injection container for list_policies feature
pub struct ListPoliciesDIContainer {
    use_case: ListPoliciesUseCase<
        SurrealListQueryValidator,
        SurrealPolicyListingStorage,
        SimplePolicyListingAuditor,
    >,
    event_handler: SimplePolicyListingEventHandler,
    config: ListPoliciesConfig,
}

impl ListPoliciesDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let config = ListPoliciesConfig::default();

        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config.clone(),
        );

        let event_handler = SimplePolicyListingEventHandler::new();

        Self {
            use_case,
            event_handler,
            config,
        }
    }

    /// Get the list policies use case
    pub fn use_case(&self) -> &ListPoliciesUseCase<
        SurrealListQueryValidator,
        SurrealPolicyListingStorage,
        SimplePolicyListingAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyListingEventHandler {
        &self.event_handler
    }

    /// Get the configuration
    pub fn config(&self) -> &ListPoliciesConfig {
        &self.config
    }
}
