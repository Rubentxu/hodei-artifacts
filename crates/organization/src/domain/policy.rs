// crates/organization/src/domain/policy.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Una Política de Control de Servicio (SCP) que establece los límites
/// de permisos para todos los principales dentro de una organización.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceControlPolicy {
    /// HRN único para la política.
    /// Formato: `hrn:hodei:iam:global:<org_id>:scp/<policy_name>`
    pub hrn: Hrn,
    
    /// HRN de la organización a la que está adjunta.
    pub organization_hrn: OrganizationId,

    /// Nombre de la política.
    pub name: String,

    /// Descripción de la política.
    pub description: Option<String>,

    /// El texto completo de la política en lenguaje Cedar.
    pub policy_text: String,

    /// Si la política está activa y se está aplicando.
    pub is_enabled: bool,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}