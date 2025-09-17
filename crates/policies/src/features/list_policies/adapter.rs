//! Concrete implementations for list_policies adapters
//!
//! This module provides concrete implementations of the segregated interfaces.

use super::error::ListPoliciesError;
use super::ports::{ListPoliciesQuery, ListQueryValidator, PolicyListingAuditor, PolicyListingStorage};
use crate::domain::ids::OrganizationId;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{debug, error, info, warn};

/// SurrealDB-based list query validator
pub struct SurrealListQueryValidator {
    max_limit: usize,
    max_offset: usize,
}

impl SurrealListQueryValidator {
    pub fn new(max_limit: usize, max_offset: usize) -> Self {
        Self {
            max_limit,
            max_offset,
        }
    }
}

#[async_trait]
impl ListQueryValidator for SurrealListQueryValidator {
    async fn validate_query(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<(), ListPoliciesError> {
        // Validate limit
        if let Some(limit) = query.limit {
            if limit > self.max_limit {
                return Err(ListPoliciesError::QueryLimitExceeded {
                    max: self.max_limit,
                    requested: limit,
                });
            }
        }

        // Validate offset
        if let Some(offset) = query.offset {
            if offset > self.max_offset {
                return Err(ListPoliciesError::invalid_query("offset", "Offset exceeds maximum allowed"));
            }
        }

        // Validate sort parameters
        if let Some(ref sort_by) = query.sort_by {
            let valid_sort_fields = ["name", "created_at", "updated_at", "status", "version"];
            if !valid_sort_fields.contains(&sort_by.as_str()) {
                return Err(ListPoliciesError::invalid_query("sort_by", "Invalid sort field"));
            }
        }

        if let Some(ref sort_order) = query.sort_order {
            if !["asc", "desc"].contains(&sort_order.as_str()) {
                return Err(ListPoliciesError::invalid_query("sort_order", "Invalid sort order"));
            }
        }

        Ok(())
    }

    async fn apply_access_filter(&self, query: &ListPoliciesQuery, user_id: &UserId) -> Result<ListPoliciesQuery, ListPoliciesError> {
        // Apply organization-based access control
        // In a real implementation, this would check user's organization membership
        // For now, return the query as-is
        Ok(query.clone())
    }
}

/// SurrealDB-based policy listing storage
pub struct SurrealPolicyListingStorage {
    db: Arc<Surreal<Any>>,
}

impl SurrealPolicyListingStorage {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PolicyListingStorage for SurrealPolicyListingStorage {
    async fn find_all(&self, query: ListPoliciesQuery) -> Result<Vec<Policy>, ListPoliciesError> {
        let mut sql = "SELECT * FROM policies WHERE status != 'deleted'".to_string();
        let mut bindings = std::collections::HashMap::new();

        // Add filters
        if let Some(ref org_id) = query.organization_id {
            sql.push_str(" AND organization_id = $org_id");
            bindings.insert("org_id".to_string(), org_id.to_string().into());
        }

        if let Some(ref name_filter) = query.name_filter {
            sql.push_str(" AND name ~ $name_filter");
            bindings.insert("name_filter".to_string(), format!("(?i){}", name_filter).into());
        }

        if let Some(ref status_filter) = query.status_filter {
            sql.push_str(" AND status = $status_filter");
            bindings.insert("status_filter".to_string(), status_filter.clone().into());
        }

        if let Some(ref created_by_filter) = query.created_by_filter {
            sql.push_str(" AND created_by = $created_by_filter");
            bindings.insert("created_by_filter".to_string(), created_by_filter.clone().into());
        }

        // Add sorting
        if let Some(ref sort_by) = query.sort_by {
            let sort_order = query.sort_order.as_deref().unwrap_or("desc");
            sql.push_str(&format!(" ORDER BY {} {}", sort_by, sort_order));
        }

        // Add pagination
        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = query.offset {
            sql.push_str(&format!(" START {}", offset));
        }

        let mut response = self.db
            .query(&sql)
            .bind(bindings)
            .await
            .map_err(|e| {
                error!("Failed to list policies: {}", e);
                ListPoliciesError::storage_error(format!("Query error: {}", e))
            })?;

        let policies: Vec<Policy> = response.take(0).map_err(|e| {
            ListPoliciesError::storage_error(format!("Response error: {}", e))
        })?;

        debug!("Retrieved {} policies", policies.len());
        Ok(policies)
    }

    async fn count(&self, query: ListPoliciesQuery) -> Result<usize, ListPoliciesError> {
        let mut sql = "SELECT count() FROM policies WHERE status != 'deleted'".to_string();
        let mut bindings = std::collections::HashMap::new();

        // Add same filters as find_all
        if let Some(ref org_id) = query.organization_id {
            sql.push_str(" AND organization_id = $org_id");
            bindings.insert("org_id".to_string(), org_id.to_string().into());
        }

        if let Some(ref name_filter) = query.name_filter {
            sql.push_str(" AND name ~ $name_filter");
            bindings.insert("name_filter".to_string(), format!("(?i){}", name_filter).into());
        }

        if let Some(ref status_filter) = query.status_filter {
            sql.push_str(" AND status = $status_filter");
            bindings.insert("status_filter".to_string(), status_filter.clone().into());
        }

        if let Some(ref created_by_filter) = query.created_by_filter {
            sql.push_str(" AND created_by = $created_by_filter");
            bindings.insert("created_by_filter".to_string(), created_by_filter.clone().into());
        }

        let mut response = self.db
            .query(&sql)
            .bind(bindings)
            .await
            .map_err(|e| {
                error!("Failed to count policies: {}", e);
                ListPoliciesError::storage_error(format!("Count error: {}", e))
            })?;

        let count_result: Option<serde_json::Value> = response.take(0).map_err(|e| {
            ListPoliciesError::storage_error(format!("Count response error: {}", e))
        })?;

        let count = count_result
            .and_then(|v| v.get("count"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as usize;

        debug!("Counted {} policies", count);
        Ok(count)
    }
}

/// Simple policy listing auditor
pub struct SimplePolicyListingAuditor;

impl SimplePolicyListingAuditor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyListingAuditor for SimplePolicyListingAuditor {
    async fn log_policy_list_access(&self, user_id: &UserId, query: &ListPoliciesQuery, result_count: usize) -> Result<(), ListPoliciesError> {
        info!("User {} listed {} policies with query: {:?}", user_id, result_count, query);
        // In a real implementation, this would log to an audit system
        Ok(())
    }
}
