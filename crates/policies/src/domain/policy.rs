use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, UserId};

use super::ids::PolicyId;

/// Política Cedar (metadatos y estado)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub current_version: PolicyVersion,
}

/// Versión de una política Cedar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersion {
    pub id: Hrn,
    pub policy_id: PolicyId,
    pub version: i64,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub created_by: UserId,
}
