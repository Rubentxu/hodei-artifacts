// crates/organization/src/domain/invitation.rs

use shared::hrn::{Hrn, OrganizationId};
use crate::domain::member::OrganizationRole;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Representa una invitación para que un usuario se una a una organización.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    /// Un token único, corto y seguro que se enviará por email. No es un HRN.
    pub token: String,
    
    /// HRN de la organización a la que se invita.
    pub organization_hrn: OrganizationId,
    
    /// Email del usuario invitado.
    pub email: String,
    
    /// Rol que se asignará al usuario si acepta.
    pub role: OrganizationRole,
    
    /// HRN del miembro que envió la invitación.
    pub inviter_hrn: Hrn,
    
    /// Estado actual de la invitación.
    pub status: InvitationStatus,
    
    /// Fecha y hora en que la invitación expira.
    pub expires_at: OffsetDateTime,
    
    /// Fecha y hora de creación.
    pub created_at: OffsetDateTime,
}

/// El estado del ciclo de vida de una invitación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Expired,
    Revoked,
}