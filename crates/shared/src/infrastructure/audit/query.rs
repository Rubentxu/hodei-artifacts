//! Query API for filtering and retrieving audit logs
//!
//! This module provides a flexible query interface for searching
//! audit logs, similar to AWS CloudWatch Logs Insights.

use super::{AuditLog, AuditLogStore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Query parameters for filtering audit logs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    /// Filter by event type (exact match)
    pub event_type: Option<String>,

    /// Filter by aggregate ID (exact match)
    pub aggregate_id: Option<String>,

    /// Filter by aggregate type (exact match)
    pub aggregate_type: Option<String>,

    /// Filter events that occurred after this time (inclusive)
    pub from_date: Option<DateTime<Utc>>,

    /// Filter events that occurred before this time (inclusive)
    pub to_date: Option<DateTime<Utc>>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Maximum number of results to return
    pub limit: Option<usize>,

    /// Number of results to skip (for pagination)
    pub offset: Option<usize>,
}

impl AuditQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by event type
    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// Filter by aggregate ID
    pub fn with_aggregate_id(mut self, aggregate_id: impl Into<String>) -> Self {
        self.aggregate_id = Some(aggregate_id.into());
        self
    }

    /// Filter by aggregate type
    pub fn with_aggregate_type(mut self, aggregate_type: impl Into<String>) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self
    }

    /// Filter by date range
    pub fn with_date_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.from_date = Some(from);
        self.to_date = Some(to);
        self
    }

    /// Filter by correlation ID
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Limit the number of results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set pagination offset
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Check if a log matches this query
    fn matches(&self, log: &AuditLog) -> bool {
        // Filter by event type
        if let Some(ref event_type) = self.event_type
            && &log.event_type != event_type
        {
            return false;
        }

        // Filter by aggregate ID
        if let Some(ref aggregate_id) = self.aggregate_id
            && log.aggregate_id.as_ref() != Some(aggregate_id)
        {
            return false;
        }

        // Filter by aggregate type
        if let Some(ref aggregate_type) = self.aggregate_type
            && log.aggregate_type.as_ref() != Some(aggregate_type)
        {
            return false;
        }

        // Filter by date range
        if let Some(from_date) = self.from_date
            && log.occurred_at < from_date
        {
            return false;
        }

        if let Some(to_date) = self.to_date
            && log.occurred_at > to_date
        {
            return false;
        }

        // Filter by correlation ID
        if let Some(ref correlation_id) = self.correlation_id
            && log.correlation_id.as_ref() != Some(correlation_id)
        {
            return false;
        }

        true
    }
}

impl AuditLogStore {
    /// Query audit logs with filters
    pub async fn query(&self, query: AuditQuery) -> Vec<AuditLog> {
        let logs = self.logs.read().await;

        let mut results: Vec<AuditLog> = logs
            .iter()
            .filter(|log| query.matches(log))
            .cloned()
            .collect();

        // Sort by occurred_at descending (newest first)
        results.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));

        // Apply pagination
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(usize::MAX);

        results.into_iter().skip(offset).take(limit).collect()
    }

    /// Count audit logs matching the query
    pub async fn count(&self, query: AuditQuery) -> usize {
        let logs = self.logs.read().await;
        logs.iter().filter(|log| query.matches(log)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use uuid::Uuid;

    fn create_test_log(
        event_type: &str,
        aggregate_id: &str,
        aggregate_type: &str,
        occurred_at: DateTime<Utc>,
    ) -> AuditLog {
        AuditLog {
            id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            aggregate_id: Some(aggregate_id.to_string()),
            aggregate_type: Some(aggregate_type.to_string()),
            event_data: serde_json::json!({}),
            occurred_at,
            correlation_id: None,
            causation_id: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_query_by_event_type() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_event_type("user.created");
        let results = store.query(query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, "user.created");
    }

    #[tokio::test]
    async fn test_query_by_aggregate_id() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;

        let query = AuditQuery::new().with_aggregate_id("user-1");
        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|r| r.aggregate_id == Some("user-1".to_string()))
        );
    }

    #[tokio::test]
    async fn test_query_by_aggregate_type() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_aggregate_type("User");
        let results = store.query(query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].aggregate_type, Some("User".to_string()));
    }

    #[tokio::test]
    async fn test_query_by_date_range() {
        let store = AuditLogStore::new();
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let two_hours_ago = now - Duration::hours(2);

        store
            .add(create_test_log("event1", "id-1", "Type1", two_hours_ago))
            .await;
        store
            .add(create_test_log("event2", "id-2", "Type2", one_hour_ago))
            .await;
        store
            .add(create_test_log("event3", "id-3", "Type3", now))
            .await;

        let query = AuditQuery::new().with_date_range(one_hour_ago, now);
        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_with_limit() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        for i in 0..10 {
            store
                .add(create_test_log(&format!("event{}", i), "id", "Type", now))
                .await;
        }

        let query = AuditQuery::new().with_limit(5);
        let results = store.query(query).await;

        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_query_with_offset() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        for i in 0..10 {
            store
                .add(create_test_log(&format!("event{}", i), "id", "Type", now))
                .await;
        }

        let query = AuditQuery::new().with_offset(5).with_limit(3);
        let results = store.query(query).await;

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_query_count() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_event_type("user.created");
        let count = store.count(query).await;

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_query_combined_filters() {
        let store = AuditLogStore::new();
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);

        store
            .add(create_test_log(
                "user.created",
                "user-1",
                "User",
                one_hour_ago,
            ))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;

        let query = AuditQuery::new()
            .with_event_type("user.created")
            .with_date_range(one_hour_ago, now)
            .with_limit(10);

        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.event_type == "user.created"));
    }
}
