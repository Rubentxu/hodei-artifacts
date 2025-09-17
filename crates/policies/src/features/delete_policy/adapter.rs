//! Concrete implementations for delete_policy adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use super::error::DeletePolicyError;
use super::ports::{DeletionMode, PolicyDeletionAuditor, PolicyDeletionRetriever, PolicyDeletionStorage, PolicyDeletionValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{debug, error, info, warn};

/// SurrealDB-based policy deletion validator
pub struct SurrealPolicyDeletionValidator;

impl SurrealPolicyDeletionValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyDeletionValidator for SurrealPolicyDeletionValidator {
    async fn validate_deletion_allowed(&self, policy: &Policy, user_id: &UserId) -> Result<(), DeletePolicyError> {
        // Check if policy is in a state that allows deletion
        if policy.status == "system" {
            return Err(DeletePolicyError::deletion_not_allowed("Cannot delete system policies"));
        }

        if policy.status == "immutable" {
            return Err(DeletePolicyError::deletion_not_allowed("Cannot delete immutable policies"));
        }

        // Additional permission checks could be added here
        Ok(())
    }

    async fn check_dependencies(&self, policy: &Policy) -> Result<(), DeletePolicyError> {
        // Check if policy has dependencies (e.g., other policies reference it)
        // This is a simplified check - in reality, you'd query for references
        if policy.name.contains("critical") {
            return Err(DeletePolicyError::has_dependencies("Policy is referenced by critical systems"));
        }

        Ok(())
    }
}

/// SurrealDB-based policy deletion retriever
pub struct SurrealPolicyDeletionRetriever {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyDeletionRetriever {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyDeletionRetriever for SurrealPolicyDeletionRetriever {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, DeletePolicyError> {
        let query = r#"
            SELECT * FROM policies WHERE id = $id AND status != "deleted"
        "#;

        let mut response = self.db
            .query(query)
            .bind(("id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to retrieve policy for deletion: {}", e);
                DeletePolicyError::storage_error(format!("Query error: {}", e))
            })?;

        let policies: Vec<Policy> = response.take(0).map_err(|e| {
            DeletePolicyError::storage_error(format!("Response error: {}", e))
        })?;

        Ok(policies.into_iter().next())
    }
}

/// SurrealDB-based policy deletion storage
pub struct SurrealPolicyDeletionStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyDeletionStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyDeletionStorage for SurrealPolicyDeletionStorage {
    async fn soft_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError> {
        let query = r#"
            UPDATE policies SET
                status = "deleted",
                deleted_at = time::now()
            WHERE id = $id
        "#;

        let mut response = self.db
            .query(query)
            .bind(("id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to soft delete policy: {}", e);
                DeletePolicyError::storage_error(format!("Soft delete error: {}", e))
            })?;

        let _: Option<serde_json::Value> = response.take(0).map_err(|e| {
            DeletePolicyError::storage_error(format!("Soft delete response error: {}", e))
        })?;

        debug!("Policy soft deleted successfully: {}", policy_id);
        Ok(())
    }

    async fn hard_delete(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError> {
        let query = "DELETE FROM policies WHERE id = $id";

        let mut response = self.db
            .query(query)
            .bind(("id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to hard delete policy: {}", e);
                DeletePolicyError::storage_error(format!("Hard delete error: {}", e))
            })?;

        let _: Option<serde_json::Value> = response.take(0).map_err(|e| {
            DeletePolicyError::storage_error(format!("Hard delete response error: {}", e))
        })?;

        debug!("Policy hard deleted successfully: {}", policy_id);
        Ok(())
    }

    async fn archive_versions(&self, policy_id: &PolicyId) -> Result<(), DeletePolicyError> {
        // Move policy versions to archive table
        let archive_query = r#"
            UPDATE policy_versions SET
                archived = true,
                archived_at = time::now()
            WHERE policy_id = $policy_id
        "#;

        let mut response = self.db
            .query(archive_query)
            .bind(("policy_id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to archive policy versions: {}", e);
                DeletePolicyError::storage_error(format!("Archive error: {}", e))
            })?;

        let _: Option<serde_json::Value> = response.take(0).map_err(|e| {
            DeletePolicyError::storage_error(format!("Archive response error: {}", e))
        })?;

        debug!("Policy versions archived successfully: {}", policy_id);
        Ok(())
    }
}

/// Simple policy deletion auditor
pub struct SimplePolicyDeletionAuditor;

impl SimplePolicyDeletionAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyDeletionAuditor for SimplePolicyDeletionAuditor {
    async fn log_policy_deletion(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), DeletePolicyError> {
        info!("Policy deleted: {} by user: {}", policy_id, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
