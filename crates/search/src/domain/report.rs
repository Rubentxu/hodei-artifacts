// crates/search/src/domain/report.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, UserId};
use time::OffsetDateTime;

/// Reporte generado por el sistema de búsqueda/analítica.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: Hrn,
    pub name: String,
    pub r#type: String,
    pub data: String,
    pub generated_at: OffsetDateTime,
    pub generated_by: UserId,
}
