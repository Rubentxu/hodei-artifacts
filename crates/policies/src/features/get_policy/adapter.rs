//! Concrete implementations for get_policy adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use super::error::GetPolicyError;
use super::ports::{PolicyAccessValidator, PolicyRetrievalAuditor, PolicyRetrievalStorage};
use crate::domain::ids::PolicyId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{debug, error, info, warn};

/// SurrealDB-based policy access validator
pub struct SurrealPolicyAccessValidator;

impl SurrealPolicyAccessValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyAccessValidator for SurrealPolicyAccessValidator {
    async fn validate_access(&self, policy: &Policy, user_id: &UserId) -> Result<(), GetPolicyError> {
        // Check if user can access this policy
        // This could involve checking organization membership, roles, etc.
        
        // For now, basic validation - could be extended
        if policy.status == "deleted" {
            return Err(GetPolicyError::policy_not_found(policy.id.clone()));
        }

        Ok(())
    }

    async fn can_read_policy(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<bool, GetPolicyError> {
        // Check if user has read permissions for the policy
        // This would typically involve checking roles and permissions
        
        // Simplified implementation
        Ok(true) // Allow read for now
    }
}

/// SurrealDB-based policy retrieval storage
pub struct SurrealPolicyRetrievalStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyRetrievalStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyRetrievalStorage for SurrealPolicyRetrievalStorage {
    async fn find_by_id(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError> {
        let query = r#"
            SELECT * FROM policies WHERE id = $id AND status != "deleted"
        "#;

        let mut response = self.db
            .query(query)
            .bind(("id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to retrieve policy: {}", e);
                GetPolicyError::storage_error(format!("Query error: {}", e))
            })?;

        let policies: Vec<Policy> = response.take(0).map_err(|e| {
            GetPolicyError::storage_error(format!("Response error: {}", e))
        })?;

        Ok(policies.into_iter().next())
    }

    async fn find_by_id_with_versions(&self, policy_id: &PolicyId) -> Result<Option<Policy>, GetPolicyError> {
        // For now, same as find_by_id - versions would be retrieved separately
        // TODO: Implement version retrieval
        self.find_by_id(policy_id).await
    }
}

/// Simple policy retrieval auditor
pub struct SimplePolicyRetrievalAuditor;

impl SimplePolicyRetrievalAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyRetrievalAuditor for SimplePolicyRetrievalAuditor {
    async fn log_policy_access(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), GetPolicyError> {
        info!("Policy accessed: {} by user: {}", policy_id, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
