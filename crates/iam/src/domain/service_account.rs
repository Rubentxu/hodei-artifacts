// crates/iam/src/domain/service_account.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Representa un principal no humano, como una aplicación o un sistema de CI/CD.
/// Es un Agregado Raíz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    /// El HRN único y global de la cuenta de servicio.
    /// Formato: `hrn:hodei:iam:global:<org_id>:service-account/<sa_name>`
    pub hrn: Hrn,

    /// La organización a la que pertenece.
    pub organization_hrn: OrganizationId,

    /// El nombre de la cuenta de servicio.
    pub name: String,

    /// Descripción de su propósito.
    pub description: Option<String>,
    
    /// El estado actual de la cuenta.
    pub status: ServiceAccountStatus,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// El estado del ciclo de vida de una cuenta de servicio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceAccountStatus { Active, Disabled }