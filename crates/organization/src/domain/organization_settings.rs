// crates/organization/src/domain/organization_settings.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, OrganizationId, UserId};
use time::OffsetDateTime;
use std::collections::HashMap;

/// Configuración a nivel de organización como entidad propia (recurso autorizable),
/// alineada con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationSettings {
    pub id: Hrn,
    pub organization: OrganizationId,
    pub settings: HashMap<String, String>,
    pub updated_at: OffsetDateTime,
    pub updated_by: UserId,
}
