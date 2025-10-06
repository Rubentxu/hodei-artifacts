//! Infrastructure Adapters (stub) for `create_policy_new` feature
//!
//! This file provides a first concrete (in-memory) implementation of the
//! `CreatePolicyPort` so the new vertical slice can be exercised end‑to‑end
//! while a real persistence layer (e.g. SurrealDB) is implemented.
//!
//! Architectural Notes
//! -------------------
//! - Respects ISP: only implements the single `create` operation required by
//!   this feature.
//! - Kept deliberately small to avoid leaking infrastructure concerns into
//!   the domain / use case layer.
//! - A future Surreal adapter will live here (or in a sibling module) and
//!   replace / complement this in-memory variant.
//!
//! Pending (Future) Work
//! ---------------------
//! - SurrealDB adapter implementing duplicate detection at the DB level.
//! - Mapping low-level storage errors into `CreatePolicyError::StorageError`.
//! - Observability: structured tracing spans for DB calls.
//! - Metrics: count created policies, latency histograms, etc.
//!
//! Testing
//! -------
//! - Unit tests for business logic are in `use_case_test.rs` using mocks,
//!   not this adapter.
//! - This adapter may be covered by integration tests once a real DB
//!   implementation exists.
//!
//! Exposure
//! --------
//! - This module is public (via feature `mod.rs`) only so the application
//!   layer DI can wire an implementation. If multiple adapters appear, you
//!   may want to re‑export only factory functions instead of concrete types.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::{instrument, warn};

use kernel::Hrn;

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::CreatePolicyPort;

// Domain-facing (lightweight) Policy representation re-used by mocks & DTO mapping
use policies::shared::domain::Policy;

/// In-memory implementation of `CreatePolicyPort`
///
/// This is a simple, thread-safe (RwLock) storage used only for development
/// & early integration. NOT for production.
///
/// Responsibilities:
/// - Enforce uniqueness of policy IDs
/// - Construct HRN for the new policy
/// - Return a fully populated `Policy` instance
///
/// Limitations:
/// - No persistence durability
/// - All data lost on process restart
/// - No indexing / query support (intentionally out of scope)
pub struct InMemoryCreatePolicyAdapter {
    /// Logical account / tenant identifier used when constructing HRNs
    account_id: String,

    /// Internal storage keyed by plain policy_id (NOT full HRN)
    store: RwLock<HashMap<String, Policy>>,
}

impl InMemoryCreatePolicyAdapter {
    /// Create a new in-memory adapter
    ///
    /// # Arguments
    /// * `account_id` - The account/tenant segment used to build policy HRNs
    pub fn new<S: Into<String>>(account_id: S) -> Self {
        Self {
            account_id: account_id.into(),
            store: RwLock::new(HashMap::new()),
        }
    }

    /// Build an HRN for the given policy id
    fn build_hrn(&self, policy_id: &str) -> Result<Hrn, CreatePolicyError> {
        let hrn_text = format!("hrn:hodei:iam::{}:policy/{}", self.account_id, policy_id);
        Hrn::from_string(&hrn_text).ok_or_else(|| {
            CreatePolicyError::InvalidHrn(format!("Invalid HRN format: {}", hrn_text))
        })
    }

    /// Check if policy id exists
    fn exists(&self, policy_id: &str) -> bool {
        self.store.read().unwrap().contains_key(policy_id)
    }
}

#[async_trait]
impl CreatePolicyPort for InMemoryCreatePolicyAdapter {
    #[instrument(
        skip(self, command),
        fields(policy_id = %command.policy_id),
        err
    )]
    async fn create(&self, command: CreatePolicyCommand) -> Result<Policy, CreatePolicyError> {
        // Basic input guard (business rules validated earlier in use case)
        if command.policy_id.trim().is_empty() {
            return Err(CreatePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        // Uniqueness check
        if self.exists(&command.policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists(
                command.policy_id.clone(),
            ));
        }

        // Construct HRN
        let hrn = self.build_hrn(&command.policy_id)?;

        // Build Policy entity using domain constructors
        let policy_id = policies::shared::domain::policy::PolicyId::new(hrn.to_string());
        let metadata =
            policies::shared::domain::policy::PolicyMetadata::new(command.description, vec![]);
        let policy = Policy::new(policy_id, command.policy_content, metadata);

        // Insert (write lock)
        let mut guard = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while writing policy");
            CreatePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        // Double-check (race condition guard) - not strictly needed in single-process test context
        if guard.contains_key(&command.policy_id) {
            return Err(CreatePolicyError::PolicyAlreadyExists(command.policy_id));
        }

        guard.insert(command.policy_id, policy.clone());

        Ok(policy)
    }
}

// -----------------------------------------------------------------------------
// Factory Helpers
// -----------------------------------------------------------------------------

/// Convenience builder returning an Arc for DI ergonomics
pub fn in_memory_adapter_arc<S: Into<String>>(account_id: S) -> Arc<InMemoryCreatePolicyAdapter> {
    Arc::new(InMemoryCreatePolicyAdapter::new(account_id))
}

// -----------------------------------------------------------------------------
// (Optional) Basic smoke tests for the adapter itself
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy_new::dto::CreatePolicyCommand;

    #[tokio::test]
    async fn creates_policy_successfully() {
        let adapter = InMemoryCreatePolicyAdapter::new("acct-123");
        let cmd = CreatePolicyCommand {
            policy_id: "p1".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let policy = adapter.create(cmd).await.unwrap();
        assert!(policy.id().to_string().contains("policy/p1"));
        assert_eq!(policy.metadata().description(), Some("Test policy"));
    }

    #[tokio::test]
    async fn rejects_duplicate_policy() {
        let adapter = InMemoryCreatePolicyAdapter::new("acct-dup");
        let cmd = CreatePolicyCommand {
            policy_id: "dup".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
        };
        adapter.create(cmd).await.unwrap();

        let duplicate = CreatePolicyCommand {
            policy_id: "dup".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
        };

        let err = adapter.create(duplicate).await.unwrap_err();
        matches!(err, CreatePolicyError::PolicyAlreadyExists(_));
    }
}
