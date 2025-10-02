// crates/iam/src/domain/events.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, OrganizationId};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `iam`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IamEvent {
    UserRegistered(UserRegistered),
    UserSuspended(UserSuspended),
    ApiKeyCreated(ApiKeyCreated),
    GroupCreated(GroupCreated),
    UserAddedToGroup(UserAddedToGroup),
    // ... otros eventos
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegistered {
    pub hrn: Hrn,
    pub email: String,
    pub organization_hrns: Vec<OrganizationId>,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSuspended {
    pub hrn: Hrn,
    pub suspended_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyCreated {
    pub hrn: Hrn,
    pub owner_hrn: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCreated {
    pub hrn: Hrn,
    pub name: String,
    pub organization_hrn: OrganizationId,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAddedToGroup {
    pub user_hrn: Hrn,
    pub group_hrn: Hrn,
    pub added_by: Hrn,
    pub at: OffsetDateTime,
}
