//! Dependency injection configuration for manage_policy_versions feature
//!
//! This module provides the dependency injection container for the manage_policy_versions use case.

use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use super::adapter::{
    SimplePolicyVersionAuditor,
    SurrealPolicyVersionHistory,
    SurrealPolicyVersionStorage,
    SurrealPolicyVersionValidator,
};
use super::event_handler::SimplePolicyVersionEventHandler;
use super::use_case::ManagePolicyVersionsUseCase;

/// Dependency injection container for manage_policy_versions feature
pub struct ManagePolicyVersionsDIContainer {
    use_case: ManagePolicyVersionsUseCase<
        SurrealPolicyVersionValidator,
        SurrealPolicyVersionHistory,
        SurrealPolicyVersionStorage,
        SimplePolicyVersionAuditor,
    >,
    event_handler: SimplePolicyVersionEventHandler,
}

impl ManagePolicyVersionsDIContainer {
    /// Create a new DI container with real dependencies
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        let event_handler = SimplePolicyVersionEventHandler::new();

        Self {
            use_case,
            event_handler,
        }
    }

    /// Get the manage policy versions use case
    pub fn use_case(&self) -> &ManagePolicyVersionsUseCase<
        SurrealPolicyVersionValidator,
        SurrealPolicyVersionHistory,
        SurrealPolicyVersionStorage,
        SimplePolicyVersionAuditor,
    > {
        &self.use_case
    }

    /// Get the event handler
    pub fn event_handler(&self) -> &SimplePolicyVersionEventHandler {
        &self.event_handler
    }
}

