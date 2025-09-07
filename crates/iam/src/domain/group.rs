// crates/iam/src/domain/group.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use shared::security::HodeiResource;
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

/// Implementación para que los grupos puedan ser parte de jerarquías en Cedar.
impl HodeiResource<EntityUid, Expr> for Group {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(&self.hrn.as_str()).unwrap()
    }
    
    fn resource_attributes(&self) -> HashMap<String, EntityUid> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), EntityUid::from_str("group").unwrap());
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de un grupo es su organización.
        vec![EntityUid::from_str(self.organization_hrn.as_str()).unwrap()]
    }
}