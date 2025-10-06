//! Dependency Injection helpers for `delete_policy` feature
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
//! use hodei_iam::features::delete_policy::di;
//!
//! # async fn example() {
//! let use_case = di::in_memory_use_case();
//! let cmd = hodei_iam::features::delete_policy::DeletePolicyCommand {
//!     policy_id: "p1".into(),
//! };
//! let _ = use_case.execute(cmd).await;
//! # }
//! ```

use std::sync::Arc;
use tracing::instrument;

use crate::features::delete_policy::adapter::InMemoryDeletePolicyAdapter;
use crate::features::delete_policy::ports::DeletePolicyPort;
use crate::features::delete_policy::use_case::DeletePolicyUseCase;

/// High-level factory providing explicit build functions.
pub struct DeletePolicyUseCaseFactory;

impl DeletePolicyUseCaseFactory {
    /// Build a use case from its already constructed dependency.
    ///
    /// This is the most generic constructor and is fully type-erased at the
    /// call site thanks to generics + inference.
    #[instrument(skip(policy_port), level = "debug")]
    pub fn build<P>(policy_port: Arc<P>) -> DeletePolicyUseCase<P>
    where
        P: DeletePolicyPort,
    {
        DeletePolicyUseCase::new(policy_port)
    }

    /// Convenience builder that owns (takes) the raw component rather than Arc
    /// and wraps it.
    #[instrument(skip(policy_port), level = "debug")]
    pub fn build_from_owned<P>(policy_port: P) -> DeletePolicyUseCase<P>
    where
        P: DeletePolicyPort + 'static,
    {
        Self::build(Arc::new(policy_port))
    }
}

/// Build a use case wired to the in-memory adapter (dev/testing).
///
/// Returns a fully constructed `DeletePolicyUseCase` ready for execution.
#[instrument(level = "debug")]
pub fn in_memory_use_case() -> DeletePolicyUseCase<InMemoryDeletePolicyAdapter> {
    let adapter = Arc::new(InMemoryDeletePolicyAdapter::new());
    DeletePolicyUseCase::new(adapter)
}

/// Build a use case with an in-memory adapter that has pre-existing policies.
///
/// This is useful for testing scenarios where you need to delete existing policies.
#[instrument(skip(policy_ids), level = "debug")]
pub fn in_memory_use_case_with_policies(
    policy_ids: Vec<String>,
) -> DeletePolicyUseCase<InMemoryDeletePolicyAdapter> {
    let adapter = Arc::new(InMemoryDeletePolicyAdapter::with_existing_policies(
        policy_ids,
    ));
    DeletePolicyUseCase::new(adapter)
}

/// Build a use case with an externally provided port implementation.
///
/// This is useful when the adapter requires complex configuration
/// (e.g. database pools) handled elsewhere in the application layer.
#[instrument(skip(policy_port), level = "debug")]
pub fn use_case_with_port<P>(policy_port: Arc<P>) -> DeletePolicyUseCase<P>
where
    P: DeletePolicyPort,
{
    DeletePolicyUseCase::new(policy_port)
}

// -----------------------------------------------------------------------------
// Test Utilities (only compiled for tests)
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::delete_policy::dto::DeletePolicyCommand;
    use crate::features::delete_policy::mocks::MockDeletePolicyPort;

    #[tokio::test]
    async fn factory_builds_use_case_and_executes_successfully() {
        let port = MockDeletePolicyPort::with_existing_policies(vec!["test-policy".to_string()]);
        let uc = DeletePolicyUseCaseFactory::build(Arc::new(port));

        let cmd = DeletePolicyCommand::new("test-policy");
        let result = uc.execute(cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn in_memory_builder_deletes_successfully() {
        let uc = in_memory_use_case_with_policies(vec!["p1".to_string(), "p2".to_string()]);

        let cmd = DeletePolicyCommand::new("p1");
        let result = uc.execute(cmd).await;
        assert!(result.is_ok());

        // Deleting again should fail (not found)
        let cmd2 = DeletePolicyCommand::new("p1");
        let result2 = uc.execute(cmd2).await;
        assert!(result2.is_err());
    }

    #[tokio::test]
    async fn in_memory_builder_rejects_non_existent() {
        let uc = in_memory_use_case();

        let cmd = DeletePolicyCommand::new("non-existent");
        let result = uc.execute(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn use_case_with_external_port_works() {
        let port = MockDeletePolicyPort::with_existing_policies(vec!["external".to_string()]);
        let uc = use_case_with_port(Arc::new(port));

        let cmd = DeletePolicyCommand::new("external");
        let result = uc.execute(cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn build_from_owned_works() {
        let port = MockDeletePolicyPort::with_existing_policies(vec!["owned".to_string()]);
        let uc = DeletePolicyUseCaseFactory::build_from_owned(port);

        let cmd = DeletePolicyCommand::new("owned");
        let result = uc.execute(cmd).await;
        assert!(result.is_ok());
    }
}
