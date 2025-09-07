// crates/security/src/domain/scan_result.rs

use shared::hrn::{Hrn, OrganizationId, PackageVersionId, VulnerabilityOccurrenceId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Representa el resultado de un único escaneo de seguridad sobre un `PackageVersion`.
/// Es un Agregado Raíz inmutable; un nuevo escaneo crea un nuevo resultado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScanResult {
    /// El HRN único del resultado del escaneo.
    /// Formato: `hrn:hodei:security:<region>:<org_id>:scan-result/<scan_id>`
    pub hrn: Hrn,

    /// La organización a la que pertenece este resultado.
    pub organization_hrn: OrganizationId,
    
    /// El HRN del `PackageVersion` que fue escaneado.
    pub package_version_hrn: PackageVersionId,

    /// El nombre de la herramienta de escaneo utilizada (ej. "Trivy", "Snyk").
    pub scanner_name: String,
    
    /// La versión de la herramienta de escaneo.
    pub scanner_version: String,
    
    /// El estado del proceso de escaneo.
    pub status: ScanStatus,
    
    /// Un resumen agregado de los hallazgos.
    pub summary: ScanSummary,

    /// Lista de HRNs a las `VulnerabilityOccurrence` encontradas en este escaneo.
    pub occurrences: Vec<VulnerabilityOccurrenceId>,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// Un resumen agregado de los hallazgos de un escaneo para una vista rápida.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub critical_count: u32,
    pub high_count: u32,
    pub medium_count: u32,
    pub low_count: u32,
    pub info_count: u32,
    pub unknown_count: u32,
    pub total: u32,
    /// Una puntuación de riesgo calculada (ej. 0.0 - 10.0).
    pub risk_score: f32,
}

/// El estado del ciclo de vida de un escaneo asíncrono.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanStatus { Pending, InProgress, Completed, Failed }