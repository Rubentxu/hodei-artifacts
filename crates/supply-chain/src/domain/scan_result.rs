// crates/supply-chain/src/domain/scan_result.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, PhysicalArtifactId};
use time::OffsetDateTime;

/// Resultado de escaneo de un artefacto físico, alineado con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub id: Hrn, // ScanResultId en el diagrama; usamos Hrn directamente como identificador
    pub artifact: PhysicalArtifactId,
    pub scanner: String,
    pub results: String,
    pub scanned_at: OffsetDateTime,
}
