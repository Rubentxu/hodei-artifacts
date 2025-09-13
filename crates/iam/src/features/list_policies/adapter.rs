// crates/iam/src/features/list_policies/adapter.rs

use crate::domain::policy::Policy;
use crate::features::list_policies::ports::PolicyLister;
use crate::features::list_policies::dto::{ListPoliciesQuery, ListPoliciesResponse};
use crate::features::list_policies::error::ListPoliciesError;
use async_trait::async_trait;
use mongodb::{bson::doc, Collection, Database};
use std::sync::Arc;

/// Adapter that implements PolicyLister using MongoDB directly
pub struct ListPoliciesAdapter {
    collection: Collection<Policy>,
}

impl ListPoliciesAdapter {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            collection: database.collection::<Policy>("policies"),
        }
    }

    /// Build MongoDB filter document from ListPoliciesQuery
    fn build_filter_document(&self, query: &ListPoliciesQuery) -> mongodb::bson::Document {
        let mut doc = doc! {};

        if let Some(ref status) = query.status {
            doc.insert("status", status.to_string());
        }

        if let Some(ref name_contains) = query.name_contains {
            doc.insert("name", doc! { "$regex": name_contains, "$options": "i" });
        }

        if !query.tags.is_empty() {
            doc.insert("tags", doc! { "$in": &query.tags });
        }

        if let Some(ref created_by) = query.created_by {
            doc.insert("metadata.created_by", created_by);
        }

        doc
    }
}

#[async_trait]
impl PolicyLister for ListPoliciesAdapter {
    async fn list(&self, query: ListPoliciesQuery) -> Result<ListPoliciesResponse, ListPoliciesError> {
        let query_doc = self.build_filter_document(&query);

        // Count total documents matching the filter
        let total_count = self
            .collection
            .count_documents(query_doc.clone())
            .await
            .map_err(|e| ListPoliciesError::DatabaseError(format!("Failed to count policies: {}", e)))?;

        // Execute the query with pagination and sorting
        let mut cursor = self
            .collection
            .find(query_doc)
            .limit(query.effective_limit() as i64)
            .skip(query.effective_offset() as u64)
            .sort(doc! { "metadata.created_at": -1 }) // Sort by creation date, newest first
            .await
            .map_err(|e| ListPoliciesError::DatabaseError(format!("Failed to list policies: {}", e)))?;

        let mut policies = Vec::new();
        while cursor
            .advance()
            .await
            .map_err(|e| ListPoliciesError::DatabaseError(format!("Failed to iterate policies: {}", e)))?
        {
            let policy = cursor.deserialize_current().map_err(|e| {
                ListPoliciesError::DatabaseError(format!("Failed to deserialize policy: {}", e))
            })?;
            policies.push(policy);
        }

        Ok(PolicyListResponse::new(policies, total_count))
    }

    async fn count(&self, query: ListPoliciesQuery) -> Result<u64, ListPoliciesError> {
        let query_doc = self.build_filter_document(&query);

        self.collection
            .count_documents(query_doc)
            .await
            .map_err(|e| ListPoliciesError::DatabaseError(format!("Failed to count policies: {}", e)))
    }
}