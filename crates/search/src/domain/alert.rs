// crates/search/src/domain/alert.rs

use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Unknown,
}

/// Alerta del sistema de búsqueda/monitoreo, alineada con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: Hrn,
    pub name: String,
    pub condition: String,
    pub severity: Severity,
    pub active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
}
