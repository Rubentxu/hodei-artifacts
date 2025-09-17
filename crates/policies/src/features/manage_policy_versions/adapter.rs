//! Concrete implementations for manage_policy_versions adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use async_trait::async_trait;
use std::sync::Arc;
use surrealdb::{engine::any::Any, Surreal};
use tracing::{debug, error, info, warn};

use super::error::ManagePolicyVersionsError;
use super::ports::{PolicyVersionAuditor, PolicyVersionHistory, PolicyVersionValidator};
use crate::domain::ids::{PolicyId, UserId};
use crate::domain::policy::{Policy, PolicyVersion};

/// SurrealDB-based policy version validator
pub struct SurrealPolicyVersionValidator;

impl SurrealPolicyVersionValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyVersionValidator for SurrealPolicyVersionValidator {
    async fn validate_version_number(&self, version: i64) -> Result<(), ManagePolicyVersionsError> {
        if version <= 0 {
            return Err(ManagePolicyVersionsError::invalid_version(version));
        }

        if version > 999999 {
            return Err(ManagePolicyVersionsError::invalid_version(version));
        }

        Ok(())
    }

    async fn validate_version_content(&self, content: &str) -> Result<(), ManagePolicyVersionsError> {
        if content.trim().is_empty() {
            return Err(ManagePolicyVersionsError::history_error("Version content cannot be empty"));
        }

        if content.len() > 10000 {
            return Err(ManagePolicyVersionsError::history_error("Version content too large"));
        }

        Ok(())
    }

    async fn validate_rollback_allowed(&self, policy: &Policy, target_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError> {
        // Check if target version exists and is valid for rollback
        if target_version >= policy.version {
            return Err(ManagePolicyVersionsError::cannot_rollback("Cannot rollback to future version"));
        }

        if policy.status == "deleted" {
            return Err(ManagePolicyVersionsError::cannot_rollback("Cannot rollback deleted policy"));
        }

        // Additional permission checks could be added here
        Ok(())
    }
}

/// SurrealDB-based policy version history manager
pub struct SurrealPolicyVersionHistory {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyVersionHistory {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyVersionHistory for SurrealPolicyVersionHistory {
    async fn get_version_history(&self, policy_id: &PolicyId, limit: Option<usize>) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError> {
        let mut sql = "SELECT * FROM policy_versions WHERE policy_id = $policy_id ORDER BY version DESC".to_string();

        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        let mut response = self.db
            .query(&sql)
            .bind(("policy_id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to get version history: {}", e);
                ManagePolicyVersionsError::storage_error(format!("Query error: {}", e))
            })?;

        let versions: Vec<PolicyVersion> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Response error: {}", e))
        })?;

        debug!("Retrieved {} versions for policy: {}", versions.len(), policy_id);
        Ok(versions)
    }

    async fn get_version_diff(&self, policy_id: &PolicyId, from_version: i64, to_version: i64) -> Result<String, ManagePolicyVersionsError> {
        // Get both versions
        let from_version_data = self.get_version_by_number(policy_id, from_version).await?
            .ok_or_else(|| ManagePolicyVersionsError::version_not_found(from_version))?;

        let to_version_data = self.get_version_by_number(policy_id, to_version).await?
            .ok_or_else(|| ManagePolicyVersionsError::version_not_found(to_version))?;

        // Simple diff - in real implementation, would use proper diff library
        let diff = format!(
            "Diff between version {} and {}:\n\nFrom (v{}):\n{}\n\nTo (v{}):\n{}",
            from_version, to_version,
            from_version, from_version_data.content,
            to_version, to_version_data.content
        );

        Ok(diff)
    }

    async fn cleanup_old_versions(&self, policy_id: &PolicyId, keep_last: usize) -> Result<(), ManagePolicyVersionsError> {
        if keep_last == 0 {
            return Err(ManagePolicyVersionsError::history_error("Must keep at least one version"));
        }

        // Get all versions except the most recent ones to keep
        let sql = r#"
            SELECT version FROM policy_versions
            WHERE policy_id = $policy_id
            ORDER BY version DESC
        "#;

        let mut response = self.db
            .query(sql)
            .bind(("policy_id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to get versions for cleanup: {}", e);
                ManagePolicyVersionsError::storage_error(format!("Query error: {}", e))
            })?;

        let versions: Vec<i64> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Response error: {}", e))
        })?;

        if versions.len() <= keep_last {
            return Ok(()); // Nothing to clean up
        }

        // Delete old versions
        let versions_to_delete: Vec<i64> = versions.into_iter().skip(keep_last).collect();

        for version in versions_to_delete {
            let delete_sql = "DELETE FROM policy_versions WHERE policy_id = $policy_id AND version = $version";

            self.db
                .query(delete_sql)
                .bind(("policy_id", policy_id.to_string()))
                .bind(("version", version))
                .await
                .map_err(|e| {
                    error!("Failed to delete version {}: {}", version, e);
                    ManagePolicyVersionsError::storage_error(format!("Delete error: {}", e))
                })?;
        }

        info!("Cleaned up old versions for policy: {}", policy_id);
        Ok(())
    }
}

impl SurrealPolicyVersionHistory {
    async fn get_version_by_number(&self, policy_id: &PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError> {
        let sql = "SELECT * FROM policy_versions WHERE policy_id = $policy_id AND version = $version";

        let mut response = self.db
            .query(sql)
            .bind(("policy_id", policy_id.to_string()))
            .bind(("version", version))
            .await
            .map_err(|e| {
                error!("Failed to get version {}: {}", version, e);
                ManagePolicyVersionsError::storage_error(format!("Query error: {}", e))
            })?;

        let versions: Vec<PolicyVersion> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Response error: {}", e))
        })?;

        Ok(versions.into_iter().next())
    }
}

/// SurrealDB-based policy version storage
pub struct SurrealPolicyVersionStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyVersionStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyVersionStorage for SurrealPolicyVersionStorage {
    async fn save_version(&self, version: PolicyVersion) -> Result<(), ManagePolicyVersionsError> {
        let query = "CREATE policy_versions CONTENT $version";
        
        self.db
            .query(query)
            .bind(("version", version))
            .await
            .map_err(|e| ManagePolicyVersionsError::storage_error(e.to_string()))?;
            
        Ok(())
    }

    async fn find_versions_by_policy(&self, policy_id: PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError> {
        let mut result = self.db
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id ORDER BY version DESC")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .map_err(|e| ManagePolicyVersionsError::storage_error(e.to_string()))?;
            
        let versions: Vec<PolicyVersion> = result.take(0)
            .map_err(|e| ManagePolicyVersionsError::storage_error(e.to_string()))?;
            
        Ok(versions)
    }

    async fn find_version(&self, policy_id: PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError> {
        let sql = "SELECT * FROM policy_versions WHERE policy_id = $policy_id AND version = $version";

        let mut response = self.db
            .query(sql)
            .bind(("policy_id", policy_id.to_string()))
            .bind(("version", version))
            .await
            .map_err(|e| {
                error!("Failed to find version {}: {}", version, e);
                ManagePolicyVersionsError::storage_error(format!("Query error: {}", e))
            })?;

        let versions: Vec<PolicyVersion> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Response error: {}", e))
        })?;

        Ok(versions.into_iter().next())
    }

    async fn delete_version(&self, policy_id: PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError> {
        let sql = "DELETE FROM policy_versions WHERE policy_id = $policy_id AND version = $version";

        let mut response = self.db
            .query(sql)
            .bind(("policy_id", policy_id.to_string()))
            .bind(("version", version))
            .await
            .map_err(|e| {
                error!("Failed to delete version {}: {}", version, e);
                ManagePolicyVersionsError::storage_error(format!("Delete error: {}", e))
            })?;

        let _: Option<serde_json::Value> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Delete response error: {}", e))
        })?;

        debug!("Version {} deleted for policy: {}", version, policy_id);
        Ok(())
    }

    async fn update_current_version(&self, policy_id: PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError> {
        // Get the version content
        let target_version = self.find_version(policy_id, version).await?
            .ok_or_else(|| ManagePolicyVersionsError::version_not_found(version))?;

        // Update the policy's current version
        let sql = r#"
            UPDATE policies SET
                version = $version,
                current_version = $current_version,
                updated_at = time::now()
            WHERE id = $policy_id
        "#;

        let mut response = self.db
            .query(sql)
            .bind(("policy_id", policy_id.to_string()))
            .bind(("version", version))
            .bind(("current_version", serde_json::to_value(&target_version).map_err(|e| {
                ManagePolicyVersionsError::storage_error(format!("Serialization error: {}", e))
            })?))
            .await
            .map_err(|e| {
                error!("Failed to update current version: {}", e);
                ManagePolicyVersionsError::storage_error(format!("Update error: {}", e))
            })?;

        let _: Option<serde_json::Value> = response.take(0).map_err(|e| {
            ManagePolicyVersionsError::storage_error(format!("Update response error: {}", e))
        })?;

        debug!("Current version updated to {} for policy: {}", version, policy_id);
        Ok(())
    }
}

#[async_trait]
pub trait PolicyVersionStorage: Send + Sync {
    async fn save_version(&self, version: PolicyVersion) -> Result<(), ManagePolicyVersionsError>;
    async fn find_versions_by_policy(&self, policy_id: PolicyId) -> Result<Vec<PolicyVersion>, ManagePolicyVersionsError>;
    async fn find_version(&self, policy_id: PolicyId, version: i64) -> Result<Option<PolicyVersion>, ManagePolicyVersionsError>;
    async fn delete_version(&self, policy_id: PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
    async fn update_current_version(&self, policy_id: PolicyId, version: i64) -> Result<(), ManagePolicyVersionsError>;
}

/// Simple policy version auditor
pub struct SimplePolicyVersionAuditor;

impl SimplePolicyVersionAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyVersionAuditor for SimplePolicyVersionAuditor {
    async fn log_version_creation(&self, policy_id: &PolicyId, version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError> {
        info!("Version {} created for policy: {} by user: {}", version, policy_id, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }

    async fn log_version_rollback(&self, policy_id: &PolicyId, from_version: i64, to_version: i64, user_id: &UserId) -> Result<(), ManagePolicyVersionsError> {
        info!("Policy {} rolled back from version {} to {} by user: {}", policy_id, from_version, to_version, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }

    async fn log_version_cleanup(&self, policy_id: &PolicyId, deleted_versions: Vec<i64>, user_id: &UserId) -> Result<(), ManagePolicyVersionsError> {
        info!("Versions {:?} cleaned up for policy: {} by user: {}", deleted_versions, policy_id, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
