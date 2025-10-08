//! Dependency Injection helpers for `create_policy` feature
//!
//! This module centralizes the assembly of the vertical slice components
//! (ports + use case) so higher layers (HTTP, CLI, jobs) don't need to know
//! about concrete implementations. It keeps construction logic cohesive and
//! testable.
//!
//! Architectural Goals
//! -------------------
//! - Encapsulate wiring logic
//! - Promote explicit dependency injection
//! - Provide factories for both production (real adapters) and tests
//! - Avoid leaking infrastructure types to callers when returning the
//!   assembled use case (callers interact with the returned use case only)
//!
//! Status
//! ------
//! Phase 2 refactor: only an in‑memory adapter is available. When a real
//! SurrealDB adapter is implemented it should be added here (without
//! modifying callers).
//!
//! Typical Usage
//! -------------
//! ```rust,ignore
//! use hodei_iam::features::create_policy::di;
//! use hodei_iam::features::create_policy::mocks::MockPolicyValidator;
//! use hodei_iam::infrastructure::in_memory::create_policy_adapter::InMemoryCreatePolicyAdapter;
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let validator = Arc::new(MockPolicyValidator::new());
//! let adapter = Arc::new(InMemoryCreatePolicyAdapter::new("acct-123".to_string()));
//! let use_case = di::use_case_with_port(adapter, validator);
//! let cmd = hodei_iam::features::create_policy::CreatePolicyCommand {
//!     policy_id: "p1".into(),
//!     policy_content: "permit(principal, action, resource);".into(),
//!     description: None,
//! };
//! let _ = use_case.execute(cmd).await;
//! # }
//! ```
//!
//! NOTE: There is also a thin factory struct (`CreatePolicyUseCaseFactory`)
//! for more generic construction scenarios.
use std::sync::Arc;
use tracing::instrument;

use crate::features::create_policy::ports::{CreatePolicyPort, PolicyValidator};
use crate::features::create_policy::use_case::CreatePolicyUseCase;
// In-memory adapter temporarily disabled - using SurrealDB for persistence
// use crate::infrastructure::in_memory::create_policy_adapter::InMemoryCreatePolicyAdapter;

/// High‑level factory providing explicit build functions.
pub struct CreatePolicyUseCaseFactory;

impl CreatePolicyUseCaseFactory {
    /// Build a use case from its already constructed dependencies.
    ///
    /// This is the most generic constructor and is fully type‑erased at the
    /// call site thanks to generics + inference.
    #[instrument(skip(policy_port, validator), level = "debug")]
    pub fn build<P, V>(policy_port: Arc<P>, validator: Arc<V>) -> CreatePolicyUseCase<P, V>
    where
        P: CreatePolicyPort,
        V: PolicyValidator,
    {
        CreatePolicyUseCase::new(policy_port, validator)
    }

    /// Convenience builder that owns (takes) the raw components rather than Arcs
    /// and wraps them.
    #[instrument(skip(policy_port, validator), level = "debug")]
    pub fn build_from_owned<P, V>(policy_port: P, validator: V) -> CreatePolicyUseCase<P, V>
    where
        P: CreatePolicyPort + 'static,
        V: PolicyValidator + 'static,
    {
        Self::build(Arc::new(policy_port), Arc::new(validator))
    }
}

/// Build a use case wired to the in‑memory adapter (dev/testing).
///
/// Returns a fully constructed `CreatePolicyUseCase` ready for execution.
// #[instrument(skip(validator), level = "debug")]
// pub fn in_memory_use_case<V>(
//     account_id: &str,
//     validator: Arc<V>,
// ) -> CreatePolicyUseCase<InMemoryCreatePolicyAdapter, V>
// where
//     V: PolicyValidator,
// {
//     let adapter = Arc::new(InMemoryCreatePolicyAdapter::new(account_id.to_string()));
//     CreatePolicyUseCase::new(adapter, validator)
// }

/// Build a use case with an externally provided port implementation.
///
/// This is useful when the adapter requires complex configuration
/// (e.g. database pools) handled elsewhere in the application layer.
#[instrument(skip(policy_port, validator), level = "debug")]
pub fn use_case_with_port<P, V>(policy_port: Arc<P>, validator: Arc<V>) -> CreatePolicyUseCase<P, V>
where
    P: CreatePolicyPort,
    V: PolicyValidator,
{
    CreatePolicyUseCase::new(policy_port, validator)
}

// -----------------------------------------------------------------------------
// Test Utilities (only compiled for tests)
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::mocks::{MockCreatePolicyPort, MockPolicyValidator};

    #[tokio::test]
    async fn factory_builds_use_case_and_executes_successfully() {
        let port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let uc = CreatePolicyUseCaseFactory::build(port.clone(), validator.clone());

        let cmd = CreatePolicyCommand {
            policy_id: "ok".into(),
            policy_content: "permit(principal, action, resource);".into(),
            description: None,
        };

        let result = uc.execute(cmd).await;
        assert!(result.is_ok());
        assert_eq!(port.get_created_count(), 1);
    }

    #[tokio::test]
    async fn in_memory_builder_creates_unique_policies() {
        let validator = Arc::new(MockPolicyValidator::new());
        let uc = in_memory_use_case("acct-test", validator);

        let cmd = CreatePolicyCommand {
            policy_id: "p1".into(),
            policy_content: "permit(principal, action, resource);".into(),
            description: None,
        };
        let first = uc.execute(cmd).await;
        assert!(first.is_ok());

        // Duplicate
        let dup = CreatePolicyCommand {
            policy_id: "p1".into(),
            policy_content: "permit(principal, action, resource);".into(),
            description: None,
        };
        let second = uc.execute(dup).await;
        assert!(second.is_err());
    }

    #[tokio::test]
    async fn use_case_with_external_port_works() {
        let port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let uc = use_case_with_port(port.clone(), validator);

        let cmd = CreatePolicyCommand {
            policy_id: "ext".into(),
            policy_content: "permit(principal, action, resource);".into(),
            description: Some("External".into()),
        };

        let view = uc.execute(cmd).await.unwrap();
        assert!(view.id.to_string().contains("policy/ext"));
        assert_eq!(port.get_created_count(), 1);
    }
}
