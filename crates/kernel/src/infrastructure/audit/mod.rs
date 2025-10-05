//! Audit system for capturing and querying domain events
//!
//! This module provides a CloudWatch-like audit logging system that captures
//! all domain events for compliance, debugging, and operational insights.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod handler;
pub mod query;

#[cfg(test)]
mod handler_test;
#[cfg(test)]
mod query_test;

// Re-export key types for convenience
pub use handler::AuditEventHandler;
pub use query::AuditQuery;

/// An audit log entry representing a captured domain event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    /// Unique identifier for this audit log entry
    pub id: Uuid,

    /// Type of the event (e.g., "iam.user.created")
    pub event_type: String,

    /// Aggregate ID that this event relates to
    pub aggregate_id: Option<String>,

    /// Type of the aggregate (e.g., "User", "Group", "Account")
    pub aggregate_type: Option<String>,

    /// The event data as JSON
    pub event_data: serde_json::Value,

    /// When the event occurred
    pub occurred_at: DateTime<Utc>,

    /// Correlation ID for tracing related events
    pub correlation_id: Option<String>,

    /// Causation ID - the ID of the command/event that caused this event
    pub causation_id: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// In-memory store for audit logs (production would use a database)
#[derive(Clone)]
pub struct AuditLogStore {
    logs: Arc<RwLock<Vec<AuditLog>>>,
}

impl AuditLogStore {
    /// Create a new empty audit log store
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a new audit log entry
    pub async fn add(&self, log: AuditLog) {
        let mut logs = self.logs.write().await;
        logs.push(log);
    }

    /// Get all audit logs (use query() for filtering)
    pub async fn all(&self) -> Vec<AuditLog> {
        let logs = self.logs.read().await;
        logs.clone()
    }

    /// Get a specific audit log by ID
    pub async fn get_by_id(&self, id: Uuid) -> Option<AuditLog> {
        let logs = self.logs.read().await;
        logs.iter().find(|log| log.id == id).cloned()
    }

    /// Count total audit logs
    pub async fn count_all(&self) -> usize {
        let logs = self.logs.read().await;
        logs.len()
    }

    /// Clear all logs (useful for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        let mut logs = self.logs.write().await;
        logs.clear();
    }
}

impl Default for AuditLogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about audit logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_events: usize,
    pub events_by_type: HashMap<String, usize>,
    pub events_by_aggregate_type: HashMap<String, usize>,
    pub oldest_event: Option<DateTime<Utc>>,
    pub newest_event: Option<DateTime<Utc>>,
}

impl AuditLogStore {
    /// Get statistics about the audit logs
    pub async fn stats(&self) -> AuditStats {
        let logs = self.logs.read().await;

        let mut events_by_type: HashMap<String, usize> = HashMap::new();
        let mut events_by_aggregate_type: HashMap<String, usize> = HashMap::new();
        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        for log in logs.iter() {
            // Count by event type
            *events_by_type.entry(log.event_type.clone()).or_insert(0) += 1;

            // Count by aggregate type
            if let Some(ref agg_type) = log.aggregate_type {
                *events_by_aggregate_type
                    .entry(agg_type.clone())
                    .or_insert(0) += 1;
            }

            // Track oldest and newest
            if oldest.is_none() || log.occurred_at < oldest.unwrap() {
                oldest = Some(log.occurred_at);
            }
            if newest.is_none() || log.occurred_at > newest.unwrap() {
                newest = Some(log.occurred_at);
            }
        }

        AuditStats {
            total_events: logs.len(),
            events_by_type,
            events_by_aggregate_type,
            oldest_event: oldest,
            newest_event: newest,
        }
    }
}
