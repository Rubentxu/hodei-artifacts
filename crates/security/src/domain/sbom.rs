use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Representa un Software Bill of Materials (SBOM) generado para un artefacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    /// ID único del SBOM.
    pub id: String,
    
    /// ID del artefacto al que pertenece este SBOM.
    pub artifact_id: String,
    
    /// Formato del SBOM.
    pub format: SbomFormat,
    
    /// Versión de la especificación del SBOM.
    pub spec_version: String,
    
    /// Contenido del SBOM en formato JSON.
    pub content: String,
    
    /// Fecha y hora de creación del SBOM.
    pub created_at: DateTime<Utc>,
    
    /// Metadatos del SBOM.
    pub metadata: SbomMetadata,
}

/// Formatos soportados para SBOM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SbomFormat {
    /// Formato CycloneDX.
    CycloneDX,
    
    /// Formato SPDX.
    SPDX,
}

/// Metadatos de un SBOM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomMetadata {
    /// Herramienta utilizada para generar el SBOM.
    pub generator: String,
    
    /// Versión de la herramienta.
    pub generator_version: String,
    
    /// Número de componentes en el SBOM.
    pub component_count: u32,
}