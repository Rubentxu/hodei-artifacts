//! Modelos de dominio Supply Chain (placeholder inicial)
use serde::{Serialize, Deserialize};
use shared::{ArtifactId, IsoTimestamp};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomSummary {
    pub sbom_id: SbomId,
    pub artifact_id: ArtifactId,
    pub generated_at: IsoTimestamp,
    pub component_count: u32,
}

/// Modelo básico SBOM para referencias en infraestructura
/// Siguiendo VSA: modelo mínimo necesario para que compile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    pub id: Uuid,
    pub artifact_id: ArtifactId,
    pub format: SbomFormat,
    pub spec_version: String,
    pub components: Vec<SbomComponent>,
    pub creation_time: chrono::DateTime<chrono::Utc>,
    pub tools: Vec<String>,
    pub document_name: String,
    pub document_namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SbomFormat {
    CycloneDx,
    Spdx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomComponent {
    pub name: String,
    pub version: String,
    pub component_type: String,
    pub licenses: Vec<String>,
    pub description: Option<String>,
    pub supplier: Option<String>,
    pub purl: Option<String>,
}

/// Modelo básico Vulnerability para referencias en infraestructura
/// Siguiendo VSA: modelo mínimo necesario para que compile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    pub id: Uuid,
    pub cve_id: Option<String>,
    pub severity: VulnerabilitySeverity,
    pub description: String,
    pub cvss_score: Option<f64>,
    pub fixed_versions: Vec<String>,
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    pub source: String,
    pub affected_components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VulnerabilitySeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Sbom {
    pub fn new(artifact_id: ArtifactId, format: SbomFormat, document_name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            artifact_id,
            format,
            spec_version: "1.4".to_string(),
            components: Vec::new(),
            creation_time: chrono::Utc::now(),
            tools: Vec::new(),
            document_name,
            document_namespace: format!("https://hodei-artifacts.com/sbom/{}", Uuid::new_v4()),
        }
    }
}

impl Vulnerability {
    pub fn new(cve_id: Option<String>, severity: VulnerabilitySeverity, description: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            cve_id,
            severity,
            description,
            cvss_score: None,
            fixed_versions: Vec::new(),
            discovered_at: chrono::Utc::now(),
            source: "unknown".to_string(),
            affected_components: Vec::new(),
        }
    }
}
