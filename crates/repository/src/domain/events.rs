// crates/repository/src/domain/events.rs

use shared::hrn::{Hrn, OrganizationId, RepositoryId};
use crate::domain::repository::{RepositoryType};
use shared::enums::Ecosystem;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `repository`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepositoryEvent {
    RepositoryCreated(RepositoryCreated),
    RepositoryUpdated(RepositoryUpdated),
    RepositoryDeleted(RepositoryDeleted),
    RetentionPolicyApplied(RetentionPolicyApplied),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCreated {
    pub hrn: RepositoryId,
    pub name: String,
    pub repo_type: RepositoryType,
    pub format: Ecosystem,
    pub organization_hrn: OrganizationId,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryUpdated {
    pub hrn: RepositoryId,
    pub name: String,
    pub repo_type: RepositoryType,
    pub format: Ecosystem,
    pub organization_hrn: OrganizationId,
    pub updated_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDeleted {
    pub hrn: RepositoryId,
    pub deleted_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicyApplied {
    pub repository_hrn: RepositoryId,
    pub policy_hrn: Hrn,
    pub result: PolicyExecutionResult,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyExecutionResult {
    pub artifacts_deleted: u32,
    pub artifacts_archived: u32,
    pub notifications_sent: u32,
    pub status: String, // "Success", "Failed"
    pub error_message: Option<String>,
}