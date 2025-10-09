//! Dependency Injection helpers for `list_policies` feature
//!
//! This module centralizes the assembly of the vertical slice components
//! (port + use case) so higher layers (HTTP, CLI, jobs) don't need to know
//! about concrete implementations. It keeps construction logic cohesive and
//! testable.
//!
//! Architectural Goals
//! -------------------
//! - Encapsulate wiring logic
//! - Promote explicit dependency injection
//! - Provide factories for both production (real adapters) and tests
//! - Avoid leaking infrastructure types to callers when returning the
//!   assembled use case
//!
//! Status
//! ------
//! Phase 2 refactor: only an in-memory adapter is available. When a real
//! SurrealDB adapter is implemented it should be added here (without
//! modifying callers).
//!
//! Typical Usage
//! -------------
//! ```rust,ignore
//! use hodei_iam::features::list_policies::di;
//!
//! # async fn example() {
//! let use_case = di::in_memory_use_case();
//! let query = hodei_iam::features::list_policies::ListPoliciesQuery::default();
//! let _ = use_case.execute(query).await;
//! # }
//! ```

use std::sync::Arc;
use tracing::instrument;

// Temporarily disabled - adapter out of sync with current ports
// use crate::infrastructure::in_memory::list_policies_adapter::InMemoryPolicyListerAdapter;
use crate::features::list_policies::ports::PolicyLister;
use crate::features::list_policies::use_case::ListPoliciesUseCase;

/// High-level factory providing explicit build functions.
pub struct ListPoliciesUseCaseFactory;

impl ListPoliciesUseCaseFactory {
    /// Build a use case from its already constructed dependency.
    ///
    /// This is the most generic constructor and is fully type-erased at the
    /// call site thanks to generics + inference.
    #[instrument(skip(policy_port), level = "debug")]
    pub fn build<P>(policy_port: Arc<P>) -> ListPoliciesUseCase<P>
    where
        P: PolicyLister,
    {
        ListPoliciesUseCase::new(policy_port)
    }

    /// Convenience builder that owns (takes) the raw component rather than Arc
    /// and wraps it.
    #[instrument(skip(policy_port), level = "debug")]
    pub fn build_from_owned<P>(policy_port: P) -> ListPoliciesUseCase<P>
    where
        P: PolicyLister + 'static,
    {
        Self::build(Arc::new(policy_port))
    }
}

/// Build a use case wired to the in-memory adapter (dev/testing).
///
/// TEMPORARILY DISABLED: In-memory adapter is out of sync with current ports.
/// Use SurrealDB adapter or mocks instead.
/// Build a use case with an externally provided port implementation.
///
/// This is useful when the adapter requires complex configuration
/// (e.g. database pools) handled elsewhere in the application layer.
#[instrument(skip(policy_port), level = "debug")]
pub fn use_case_with_port<P>(policy_port: Arc<P>) -> ListPoliciesUseCase<P>
where
    P: PolicyLister,
{
    ListPoliciesUseCase::new(policy_port)
}

// -----------------------------------------------------------------------------
// Test Utilities (only compiled for tests)
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::list_policies::dto::ListPoliciesQuery;
    use crate::features::list_policies::mocks::MockPolicyLister;

    #[tokio::test]
    async fn factory_builds_use_case_and_executes_successfully() {
        let port = MockPolicyLister::new();
        let uc = ListPoliciesUseCaseFactory::build(Arc::new(port));

        let query = ListPoliciesQuery::default();
        let result = uc.execute(query).await;
        assert!(result.is_ok());
    }

    // Temporarily disabled - in-memory adapter out of sync
    /*
    #[tokio::test]
    async fn in_memory_builder_lists_policies_successfully() {
        let uc = in_memory_use_case();

        // Add test policies to the adapter
        let adapter = Arc::new(InMemoryPolicyListerAdapter::new());
        adapter.add_policies(vec![
            ("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            ("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ]);

        let uc_with_data = ListPoliciesUseCase::new(adapter);
        let query = ListPoliciesQuery::default();
        let result = uc_with_data.execute(query).await;
        assert!(result.is_ok());
    }
    */

    #[tokio::test]
    async fn use_case_with_external_port_works() {
        let port = MockPolicyLister::new();
        let uc = use_case_with_port(Arc::new(port));

        let query = ListPoliciesQuery::default();
        let result = uc.execute(query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn build_from_owned_works() {
        let port = MockPolicyLister::new();
        let uc = ListPoliciesUseCaseFactory::build_from_owned(port);

        let query = ListPoliciesQuery::default();
        let result = uc.execute(query).await;
        assert!(result.is_ok());
    }
}
