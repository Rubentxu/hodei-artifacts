// crates/search/src/domain/dashboard.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{DashboardId, OrganizationId};
use time::OffsetDateTime;
use std::collections::HashSet;

/// Dashboard de métricas/búsqueda, alineado con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: DashboardId,
    pub name: String,
    pub organization: OrganizationId,
    pub widgets: HashSet<String>,
    pub public: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
}
