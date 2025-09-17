//! Domain events for the policies bounded context

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use shared::hrn::{OrganizationId, HodeiPolicyId, UserId};

/// Emitted when a policy has been created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCreatedEvent {
    pub policy_id: HodeiPolicyId,
    pub organization_id: OrganizationId,
    pub created_by: UserId,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// Emitted when a policy has been updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdatedEvent {
    pub policy_id: HodeiPolicyId,
    pub updated_by: UserId,
    pub changes: Vec<String>,
    pub new_version: i64,
    pub updated_at: DateTime<Utc>,
}

/// Emitted when a policy has been deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDeletedEvent {
    pub policy_id: HodeiPolicyId,
    pub deleted_by: UserId,
    pub deletion_mode: crate::features::delete_policy::ports::DeletionMode,
    pub deleted_at: DateTime<Utc>,
}

/// Emitted when policies are listed by a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoliciesListedEvent {
    pub listed_by: UserId,
    pub result_count: u32,
    pub query_filters: HashMap<String, String>,
    pub listed_at: DateTime<Utc>,
}
