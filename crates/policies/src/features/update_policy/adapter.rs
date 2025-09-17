//! Concrete implementations for update_policy adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use super::error::UpdatePolicyError;
use super::ports::{PolicyRetriever, PolicyUpdateAuditor, PolicyUpdateStorage, PolicyUpdateValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use cedar_policy::Policy as CedarPolicy;
use shared::hrn::UserId;
use std::str::FromStr;
use std::sync::Arc;
use surrealdb::{engine::any::Any, Surreal};
use tracing::{debug, error, info, warn};

/// SurrealDB-based policy update validator
pub struct SurrealPolicyUpdateValidator;

impl SurrealPolicyUpdateValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyUpdateValidator for SurrealPolicyUpdateValidator {
    async fn validate_policy_content(&self, content: &str) -> Result<(), UpdatePolicyError> {
        if content.trim().is_empty() {
            return Err(UpdatePolicyError::validation_failed("Policy content cannot be empty"));
        }

        if content.len() > 10000 {
            return Err(UpdatePolicyError::validation_failed("Policy content too large"));
        }

        Ok(())
    }

    async fn validate_policy_syntax(&self, content: &str) -> Result<(), UpdatePolicyError> {
        // Parse the Cedar policy to validate syntax
        match CedarPolicy::from_str(content) {
            Ok(_) => Ok(()),
            Err(e) => Err(UpdatePolicyError::validation_failed(format!("Syntax error: {}", e))),
        }
    }

    async fn validate_policy_semantics(&self, content: &str, policy_id: &PolicyId) -> Result<(), UpdatePolicyError> {
        // Basic semantic validation - could be extended
        if !content.contains("permit") && !content.contains("forbid") {
            return Err(UpdatePolicyError::validation_failed("Policy must contain permit or forbid statements"));
        }

        Ok(())
    }

    async fn validate_update_allowed(&self, policy: &Policy, user_id: &UserId) -> Result<(), UpdatePolicyError> {
        // Check if policy is in a state that allows updates
        if policy.status == "archived" {
            return Err(UpdatePolicyError::update_not_allowed("Cannot update archived policy"));
        }

        if policy.status == "deleted" {
            return Err(UpdatePolicyError::update_not_allowed("Cannot update deleted policy"));
        }

        // Additional permission checks could be added here
        Ok(())
    }
}

/// SurrealDB-based policy retriever
pub struct SurrealPolicyRetriever {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyRetriever {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyRetriever for SurrealPolicyRetriever {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, UpdatePolicyError> {
        let query = r#"
            SELECT * FROM policies WHERE id = $id
        "#;

        let mut response = self.db
            .query(query)
            .bind(("id", policy_id.to_string()))
            .await
            .map_err(|e| {
                error!("Failed to retrieve policy: {}", e);
                UpdatePolicyError::storage_error(format!("Query error: {}", e))
            })?;

        let policies: Vec<Policy> = response.take(0).map_err(|e| {
            UpdatePolicyError::storage_error(format!("Response error: {}", e))
        })?;

        Ok(policies.into_iter().next())
    }
}

/// SurrealDB-based policy update storage
pub struct SurrealPolicyUpdateStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyUpdateStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyUpdateStorage for SurrealPolicyUpdateStorage {
    async fn update(&self, policy: &Policy) -> Result<(), UpdatePolicyError> {
        let query = "UPDATE policies SET name = $name, description = $description, status = $status, version = $version, current_version = $current_version WHERE id = $id";
        
        self.db
            .query(query)
            .bind(("id", policy.id.to_string()))
            .bind(("name", policy.name.clone()))
            .bind(("description", policy.description.clone()))
            .bind(("status", policy.status.clone()))
            .bind(("version", policy.version))
            .bind(("current_version", policy.current_version.clone()))
            .await
            .map_err(|e| UpdatePolicyError::storage_error(e.to_string()))?;
            
        Ok(())
    }

    async fn create_version(&self, version: &PolicyVersion) -> Result<(), UpdatePolicyError> {
        let query = "CREATE policy_versions CONTENT $version";
        
        self.db
            .query(query)
            .bind(("version", version))
            .await
            .map_err(|e| UpdatePolicyError::storage_error(e.to_string()))?;
            
        Ok(())
    }
}

/// Simple policy update auditor
pub struct SimplePolicyUpdateAuditor;

impl SimplePolicyUpdateAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyUpdateAuditor for SimplePolicyUpdateAuditor {
    async fn log_policy_update(&self, policy_id: &PolicyId, user_id: &UserId, changes: Vec<String>) -> Result<(), UpdatePolicyError> {
        info!("Policy updated: {} by user: {}, changes: {:?}", policy_id, user_id, changes);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
