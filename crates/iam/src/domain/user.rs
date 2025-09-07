// crates/iam/src/domain/user.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use shared::security::HodeiResource;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Representa a un usuario humano, un principal fundamental en el sistema.
/// Es un Agregado Raíz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// El HRN único y global del usuario.
    /// Formato: `hrn:hodei:iam:global:<org_id>:user/<user_id>`
    pub hrn: Hrn,

    /// El email del usuario, usado para login y notificaciones. Debe ser único.
    pub email: String,

    /// El estado actual de la cuenta de usuario.
    pub status: UserStatus,

    /// Información de perfil adicional y no crítica para la seguridad.
    pub profile: UserProfile,
    
    /// Lista de HRNs de las organizaciones a las que este usuario pertenece.
    /// Esta información es crucial para las políticas de Cedar.
    pub organization_memberships: Vec<OrganizationId>,

    /// Lista de HRNs de los grupos a los que este usuario pertenece.
    pub group_memberships: Vec<Hrn>,

    /// El ID del usuario en un proveedor de identidad externo (ej. Keycloak, Okta).
    pub external_id: Option<String>,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// Información de perfil de un usuario.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    /// El nombre completo del usuario.
    pub full_name: Option<String>,
    /// La URL a la imagen de avatar del usuario.
    pub avatar_url: Option<String>,
}

/// El estado del ciclo de vida de una cuenta de usuario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    /// El usuario puede autenticarse y operar.
    Active,
    /// El usuario no puede autenticarse.
    Suspended,
    /// La cuenta está pendiente de eliminación.
    PendingDeletion,
}

