// crates/organization/src/domain/organization.rs

use shared::hrn::{Hrn, UserId};
use shared::lifecycle::{Lifecycle};
use shared::security::HodeiResource;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use cedar_policy::{EntityUid, RestrictedExpression};
use std::str::FromStr;
use std::collections::HashMap;

/// Representa un tenant del sistema, un contenedor lógico que aísla todos sus recursos.
/// Es el Agregado Raíz principal de este Bounded Context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    /// El identificador único y global de la organización.
    /// Formato: `hrn:hodei:iam:global:<org_id>:organization`
    pub hrn: Hrn,

    /// El nombre legible de la organización.
    pub name: String,

    /// El HRN del usuario que es el propietario actual de la organización.
    /// El creador original se encuentra en `lifecycle.created_by`.
    pub current_owner_hrn: UserId,
    
    /// La región geográfica principal de la organización.
    pub primary_region: String,

    /// El estado actual de la organización.
    pub status: OrganizationStatus,

    /// Configuraciones específicas del tenant.
    pub settings: OrganizationSettings,

    /// Información de auditoría y ciclo de vida (creación, actualización, estado).
    pub lifecycle: Lifecycle,
}

/// Configuraciones a nivel de organización.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationSettings {
    /// Email de contacto para asuntos de facturación y administrativos.
    pub billing_contact_email: String,
    
    /// El nivel de permiso por defecto para nuevos repositorios creados en la organización.
    pub default_repository_permission: String, // ej. "read", "none"
}

/// El estado del ciclo de vida de una organización.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrganizationStatus {
    /// La organización está activa y plenamente funcional.
    Active,
    /// La organización ha sido suspendida, bloqueando el acceso a sus recursos.
    Suspended,
    /// La organización está en proceso de ser eliminada permanentemente.
    PendingDeletion,
}

/// Implementación del trait `HodeiResource<EntityUid>` para que las organizaciones
/// puedan ser utilizadas en políticas de autorización con Cedar.
impl HodeiResource<EntityUid, RestrictedExpression> for Organization {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(&self.hrn.as_str()).unwrap()
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("organization".to_string()));
        attrs.insert("status".to_string(), RestrictedExpression::new_string(format!("{:?}", self.status)));
        attrs.insert("primary_region".to_string(), RestrictedExpression::new_string(self.primary_region.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Las organizaciones son la raíz de la jerarquía, no tienen padres.
        vec![]
    }
}