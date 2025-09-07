// crates/iam/src/domain/group.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Representa un grupo de principals (usuarios o cuentas de servicio) dentro de una organización.
/// Facilita la asignación de permisos a múltiples principals a la vez.
/// Es un Agregado Raíz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// El HRN único y global del grupo.
    /// Formato: `hrn:hodei:iam:global:<org_id>:group/<group_name>`
    pub hrn: Hrn,

    /// La organización a la que pertenece este grupo.
    pub organization_hrn: OrganizationId,
    
    /// El nombre del grupo, único dentro de la organización.
    pub name: String,
    
    /// Descripción del propósito del grupo.
    pub description: Option<String>,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

