// crates/organization/src/domain/events.rs

use shared::hrn::{Hrn, OrganizationId, UserId};
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `organization`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrganizationEvent {
    /// Se ha creado una nueva organización.
    OrganizationCreated(OrganizationCreated),
    
    /// Un miembro ha sido invitado a unirse a una organización.
    MemberInvited(MemberInvited),
    
    /// Un usuario ha aceptado una invitación y se ha unido a una organización.
    MemberJoined(MemberJoined),
    
    /// Se ha actualizado una Política de Control de Servicio.
    ScpUpdated(ScpUpdated),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationCreated {
    pub hrn: OrganizationId,
    pub name: String,
    pub owner_hrn: UserId,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInvited {
    pub invitation_token: String,
    pub organization_hrn: OrganizationId,
    pub email: String,
    pub inviter_hrn: UserId,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberJoined {
    pub member_hrn: Hrn,
    pub organization_hrn: OrganizationId,
    pub user_hrn: UserId,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpUpdated {
    pub policy_hrn: Hrn,
    pub updated_by: Hrn,
    pub at: OffsetDateTime,
}