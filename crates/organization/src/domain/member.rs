// crates/organization/src/domain/member.rs

use shared::hrn::{Hrn, UserId, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Representa la relación entre un `User` y una `Organization`, incluyendo sus roles.
/// Es una entidad dentro del agregado `Organization`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    /// HRN único para la membresía en sí.
    /// Formato: `hrn:hodei:iam:global:<org_id>:member/<user_id>`
    pub hrn: Hrn,

    /// El HRN del usuario.
    pub user_hrn: UserId,

    /// El HRN de la organización.
    pub organization_hrn: OrganizationId,

    /// Roles asignados al usuario dentro de esta organización.
    pub roles: Vec<OrganizationRole>,

    /// Información de auditoría y ciclo de vida de la membresía.
    pub lifecycle: Lifecycle,
}

/// Roles que un miembro puede tener dentro de una organización.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrganizationRole {
    /// Permisos completos sobre la organización.
    Admin,
    /// Permisos estándar para miembros del equipo.
    Member,
    /// Permisos para gestionar la facturación y suscripción.
    BillingManager,
}