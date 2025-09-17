//! Concrete implementations for create_policy adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use super::error::CreatePolicyError;
use super::ports::{PolicyCreationAuditor, PolicyCreationStorage, PolicyCreationValidator, PolicyExistenceChecker};
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use cedar_policy::Policy as CedarPolicy;
use shared::hrn::UserId;
use std::str::FromStr;
use std::sync::Arc;
use surrealdb::{engine::any::Any, Surreal};
use tracing::{debug, error, info};

/// SurrealDB-based policy creation validator
pub struct SurrealPolicyCreationValidator;

impl SurrealPolicyCreationValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyCreationValidator for SurrealPolicyCreationValidator {

    async fn validate_policy_content(&self, content: &str) -> Result<(), CreatePolicyError> {
        if content.trim().is_empty() {
            return Err(CreatePolicyError::validation_failed("Policy content cannot be empty"));
        }

        if content.len() > 10000 {
            return Err(CreatePolicyError::validation_failed("Policy content too large"));
        }

        Ok(())
    }

    async fn validate_policy_syntax(&self, content: &str) -> Result<(), CreatePolicyError> {
        // Parse the Cedar policy to validate syntax
        match CedarPolicy::from_str(content) {
            Ok(_) => Ok(()),
            Err(e) => Err(CreatePolicyError::validation_failed(format!("Syntax error: {}", e))),
        }
    }

    async fn validate_policy_semantics(&self, content: &str, policy_id: &PolicyId) -> Result<(), CreatePolicyError> {
        // Basic semantic validation - could be extended
        if !content.contains("permit") && !content.contains("forbid") {
            return Err(CreatePolicyError::validation_failed("Policy must contain permit or forbid statements"));
        }

        Ok(())
    }
}

/// SurrealDB-based policy existence checker
pub struct SurrealPolicyExistenceChecker {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyExistenceChecker {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyExistenceChecker for SurrealPolicyExistenceChecker {
    async fn exists(&self, policy_id: &PolicyId) -> Result<bool, CreatePolicyError> {
        let query = format!("SELECT id FROM policies WHERE id = '{}'", policy_id);

        match self.db.query(&query).await {
            Ok(mut response) => {
                let result: Option<serde_json::Value> = response.take(0).map_err(|e| {
                    error!("Failed to check policy existence: {}", e);
                    CreatePolicyError::storage_error(format!("Query error: {}", e))
                })?;

                Ok(result.is_some())
            }
            Err(e) => {
                error!("Database error checking policy existence: {}", e);
                Err(CreatePolicyError::storage_error(format!("Database error: {}", e)))
            }
        }
    }
}

/// SurrealDB-based policy creation storage
pub struct SurrealPolicyCreationStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyCreationStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyCreationStorage for SurrealPolicyCreationStorage {
    async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError> {
        // Extract fields to avoid holding reference across await
        let id = policy.id.clone();
        let name = policy.name.clone();
        let description = policy.description.clone();
        let status = policy.status.clone();
        let version = policy.version;
        let created_at = policy.created_at;
        let updated_at = policy.updated_at;
        let current_version = policy.current_version.clone();
        
        let query = "CREATE policies CONTENT {
            id: $id,
            name: $name,
            description: $description,
            status: $status,
            version: $version,
            created_at: $created_at,
            updated_at: $updated_at,
            current_version: $current_version
        }";
        
        self.db
            .query(query)
            .bind(("id", id.to_string()))
            .bind(("name", name))
            .bind(("description", description))
            .bind(("status", status))
            .bind(("version", version))
            .bind(("created_at", created_at))
            .bind(("updated_at", updated_at))
            .bind(("current_version", current_version))
            .await
            .map_err(|e| CreatePolicyError::storage_error(e.to_string()))?;
            
        Ok(())
    }

    async fn create_version(&self, version: &PolicyVersion) -> Result<(), CreatePolicyError> {
        // Similarly extract fields for version
        let id = version.id.clone();
        let policy_id = version.policy_id.clone();
        let version_num = version.version;
        let content = version.content.clone();
        let created_at = version.created_at;
        let created_by = version.created_by.clone();
        
        let query = "CREATE policy_versions CONTENT {
            id: $id,
            policy_id: $policy_id,
            version: $version,
            content: $content,
            created_at: $created_at,
            created_by: $created_by
        }";
        
        self.db
            .query(query)
            .bind(("id", id.to_string()))
            .bind(("policy_id", policy_id.to_string()))
            .bind(("version", version_num))
            .bind(("content", content))
            .bind(("created_at", created_at))
            .bind(("created_by", created_by.to_string()))
            .await
            .map_err(|e| CreatePolicyError::storage_error(e.to_string()))?;
            
        Ok(())
    }
}

/// Simple policy creation auditor
pub struct SimplePolicyCreationAuditor;

impl SimplePolicyCreationAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyCreationAuditor for SimplePolicyCreationAuditor {
    async fn log_policy_creation(&self, policy_id: &PolicyId, user_id: &UserId) -> Result<(), CreatePolicyError> {
        info!("Policy created: {} by user: {}", policy_id, user_id);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
