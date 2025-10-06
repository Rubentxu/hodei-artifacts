//! Infrastructure Adapters for `delete_policy` feature
//!
//! This file provides a concrete (in-memory) implementation of the
//! `DeletePolicyPort` so the vertical slice can be exercised end-to-end
//! while a real persistence layer (e.g. SurrealDB) is implemented.
//!
//! Architectural Notes
//! -------------------
//! - Respects ISP: only implements the single `delete` operation required by
//!   this feature.
//! - Kept deliberately small to avoid leaking infrastructure concerns into
//!   the domain / use case layer.
//! - A future Surreal adapter will live here (or in a sibling module) and
//!   replace / complement this in-memory variant.
//!
//! Pending (Future) Work
//! ---------------------
//! - SurrealDB adapter implementing policy existence checks at the DB level.
//! - Policy usage tracking (check if policy is attached to users/groups/roles).
//! - Mapping low-level storage errors into `DeletePolicyError::StorageError`.
//! - Observability: structured tracing spans for DB calls.
//! - Metrics: count deleted policies, latency histograms, etc.
//! - Soft delete support (marking as deleted vs. hard deletion).
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
//!   layer DI can wire an implementation.

use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::{info, instrument, warn};

use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::ports::DeletePolicyPort;

/// In-memory implementation of `DeletePolicyPort`
///
/// This is a simple, thread-safe (RwLock) storage used only for development
/// & early integration. NOT for production.
///
/// Responsibilities:
/// - Check if policy exists before deletion
/// - Remove policy from in-memory storage
/// - Track system-protected policies
/// - Track policies in use (optional safety check)
///
/// Limitations:
/// - No persistence durability
/// - All data lost on process restart
/// - No audit trail of deletions
/// - Simplified policy usage tracking (real impl would check actual attachments)
pub struct InMemoryDeletePolicyAdapter {
    /// Internal storage of existing policy IDs (keyed by plain policy_id, NOT full HRN)
    store: RwLock<HashSet<String>>,

    /// Policy IDs that are system-protected and cannot be deleted
    system_protected_ids: HashSet<String>,

    /// Policy IDs that are marked as "in use" (attached to users/groups/roles)
    /// In a real implementation, this would be dynamically checked via joins/queries
    in_use_policy_ids: RwLock<HashSet<String>>,
}

impl InMemoryDeletePolicyAdapter {
    /// Create a new in-memory delete adapter with no policies
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashSet::new()),
            system_protected_ids: HashSet::new(),
            in_use_policy_ids: RwLock::new(HashSet::new()),
        }
    }

    /// Create a new adapter with pre-existing policy IDs
    pub fn with_existing_policies(policy_ids: Vec<String>) -> Self {
        let mut store = HashSet::new();
        for id in policy_ids {
            store.insert(id);
        }
        Self {
            store: RwLock::new(store),
            system_protected_ids: HashSet::new(),
            in_use_policy_ids: RwLock::new(HashSet::new()),
        }
    }

    /// Create a new adapter with system-protected policies
    pub fn with_system_protected(policy_ids: Vec<String>) -> Self {
        let mut store = HashSet::new();
        let mut protected = HashSet::new();
        for id in policy_ids {
            store.insert(id.clone());
            protected.insert(id);
        }
        Self {
            store: RwLock::new(store),
            system_protected_ids: protected,
            in_use_policy_ids: RwLock::new(HashSet::new()),
        }
    }

    /// Add a policy to the store (for testing/setup)
    pub fn add_policy(&self, policy_id: String) {
        self.store.write().unwrap().insert(policy_id);
    }

    /// Mark a policy as "in use" (for testing/setup)
    pub fn mark_as_in_use(&self, policy_id: String) {
        self.in_use_policy_ids.write().unwrap().insert(policy_id);
    }

    /// Check if policy exists
    pub fn exists(&self, policy_id: &str) -> bool {
        self.store.read().unwrap().contains(policy_id)
    }

    /// Get the count of policies in storage
    pub fn count(&self) -> usize {
        self.store.read().unwrap().len()
    }
}

impl Default for InMemoryDeletePolicyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeletePolicyPort for InMemoryDeletePolicyAdapter {
    #[instrument(
        skip(self),
        fields(policy_id = %policy_id),
        err
    )]
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        info!("Attempting to delete policy: {}", policy_id);

        // Basic input guard (business rules validated earlier in use case)
        if policy_id.trim().is_empty() {
            return Err(DeletePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }

        // Check if policy is system-protected
        if self.system_protected_ids.contains(policy_id) {
            warn!("Attempted to delete system-protected policy: {}", policy_id);
            return Err(DeletePolicyError::SystemPolicyProtected(
                policy_id.to_string(),
            ));
        }

        // Check if policy exists
        let exists = self.store.read().map_err(|_| {
            warn!("RwLock poisoned while reading policy store");
            DeletePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        if !exists.contains(policy_id) {
            warn!("Policy not found: {}", policy_id);
            return Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()));
        }

        drop(exists); // Release read lock before checking in-use status

        // Check if policy is in use
        let in_use = self.in_use_policy_ids.read().map_err(|_| {
            warn!("RwLock poisoned while reading in-use policy list");
            DeletePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        if in_use.contains(policy_id) {
            warn!("Policy is in use and cannot be deleted: {}", policy_id);
            return Err(DeletePolicyError::PolicyInUse(format!(
                "Policy '{}' is attached to users, groups, or roles",
                policy_id
            )));
        }

        drop(in_use); // Release read lock before acquiring write lock

        // Delete the policy (acquire write lock)
        let mut store = self.store.write().map_err(|_| {
            warn!("RwLock poisoned while writing to policy store");
            DeletePolicyError::StorageError("Internal storage lock poisoned".to_string())
        })?;

        // Double-check existence (race condition guard)
        if !store.contains(policy_id) {
            return Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()));
        }

        store.remove(policy_id);
        info!("Policy deleted successfully: {}", policy_id);

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Factory Helpers
// -----------------------------------------------------------------------------

/// Convenience builder returning an Arc for DI ergonomics
pub fn in_memory_adapter_arc() -> Arc<InMemoryDeletePolicyAdapter> {
    Arc::new(InMemoryDeletePolicyAdapter::new())
}

/// Convenience builder with existing policies
pub fn in_memory_adapter_with_policies(
    policy_ids: Vec<String>,
) -> Arc<InMemoryDeletePolicyAdapter> {
    Arc::new(InMemoryDeletePolicyAdapter::with_existing_policies(
        policy_ids,
    ))
}

// -----------------------------------------------------------------------------
// Basic smoke tests for the adapter itself
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deletes_existing_policy_successfully() {
        let adapter = InMemoryDeletePolicyAdapter::with_existing_policies(vec![
            "policy1".to_string(),
            "policy2".to_string(),
        ]);

        assert!(adapter.exists("policy1"));
        assert_eq!(adapter.count(), 2);

        adapter.delete("policy1").await.unwrap();

        assert!(!adapter.exists("policy1"));
        assert!(adapter.exists("policy2"));
        assert_eq!(adapter.count(), 1);
    }

    #[tokio::test]
    async fn rejects_deletion_of_non_existent_policy() {
        let adapter = InMemoryDeletePolicyAdapter::new();

        let err = adapter.delete("non-existent").await.unwrap_err();
        matches!(err, DeletePolicyError::PolicyNotFound(_));
    }

    #[tokio::test]
    async fn rejects_deletion_of_system_protected_policy() {
        let adapter =
            InMemoryDeletePolicyAdapter::with_system_protected(vec!["admin-policy".to_string()]);

        let err = adapter.delete("admin-policy").await.unwrap_err();
        matches!(err, DeletePolicyError::SystemPolicyProtected(_));
        assert!(adapter.exists("admin-policy")); // Policy still exists
    }

    #[tokio::test]
    async fn rejects_deletion_of_policy_in_use() {
        let adapter =
            InMemoryDeletePolicyAdapter::with_existing_policies(vec!["active-policy".to_string()]);
        adapter.mark_as_in_use("active-policy".to_string());

        let err = adapter.delete("active-policy").await.unwrap_err();
        matches!(err, DeletePolicyError::PolicyInUse(_));
        assert!(adapter.exists("active-policy")); // Policy still exists
    }

    #[tokio::test]
    async fn handles_empty_policy_id() {
        let adapter = InMemoryDeletePolicyAdapter::new();

        let err = adapter.delete("").await.unwrap_err();
        matches!(err, DeletePolicyError::InvalidPolicyId(_));
    }

    #[tokio::test]
    async fn multiple_deletions_work_correctly() {
        let adapter = InMemoryDeletePolicyAdapter::with_existing_policies(vec![
            "p1".to_string(),
            "p2".to_string(),
            "p3".to_string(),
        ]);

        adapter.delete("p1").await.unwrap();
        adapter.delete("p3").await.unwrap();

        assert!(!adapter.exists("p1"));
        assert!(adapter.exists("p2"));
        assert!(!adapter.exists("p3"));
        assert_eq!(adapter.count(), 1);
    }

    #[tokio::test]
    async fn add_policy_and_delete_works() {
        let adapter = InMemoryDeletePolicyAdapter::new();
        adapter.add_policy("new-policy".to_string());

        assert!(adapter.exists("new-policy"));
        assert_eq!(adapter.count(), 1);

        adapter.delete("new-policy").await.unwrap();

        assert!(!adapter.exists("new-policy"));
        assert_eq!(adapter.count(), 0);
    }
}
