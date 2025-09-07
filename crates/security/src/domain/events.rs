// crates/security/src/domain/events.rs

use shared::hrn::{Hrn, PackageVersionId, ScanResultId};
use shared::enums::VulnerabilitySeverity;
use crate::domain::scan_result::ScanSummary;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `security`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityEvent {
    /// Se solicita un nuevo escaneo para un artefacto.
    /// Puede ser consumido por un orquestador de escaneos.
    ScanRequested(ScanRequested),

    /// Un escaneo se ha completado.
    ScanCompleted(ScanCompleted),

    /// Se ha encontrado una vulnerabilidad de alta criticidad para notificación inmediata.
    CriticalVulnerabilityFound(CriticalVulnerabilityFound),
    
    /// Una nueva definición de vulnerabilidad ha sido añadida a la base de datos.
    VulnerabilityDefinitionAdded(VulnerabilityDefinitionAdded),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequested {
    pub package_version_hrn: PackageVersionId,
    pub requested_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCompleted {
    pub hrn: ScanResultId,
    pub package_version_hrn: PackageVersionId,
    pub summary: ScanSummary,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalVulnerabilityFound {
    pub occurrence_hrn: Hrn,
    pub package_version_hrn: PackageVersionId,
    pub vulnerability_id: String, // ej. "CVE-2021-44228"
    pub severity: VulnerabilitySeverity, // Siempre será 'Critical'
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityDefinitionAdded {
    pub hrn: Hrn,
    pub source_id: String,
    pub source: String,
    pub severity: VulnerabilitySeverity,
    pub at: OffsetDateTime,
}