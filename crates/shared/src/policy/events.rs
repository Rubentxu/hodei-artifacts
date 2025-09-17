use serde::{Deserialize, Serialize};
use crate::hrn::PolicyId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCreated {
    pub id: PolicyId,
    pub name: String,
    pub version: i64,
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUpdated {
    pub id: PolicyId,
    pub from_version: i64,
    pub to_version: i64,
    pub updated_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionSummary {
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluated {
    pub id: PolicyId,
    pub context_hash: String,
    pub decision: DecisionSummary,
    pub evaluation_time_ms: u64,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyDomainEvent {
    Created(PolicyCreated),
    Updated(PolicyUpdated),
    Evaluated(PolicyEvaluated),
}

// Mapping into a cross-domain DomainEvent envelope will be added when the bus is wired.
