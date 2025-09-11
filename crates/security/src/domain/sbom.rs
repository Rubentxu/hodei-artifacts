use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use shared::hrn::Hrn;

/// Representa un Software Bill of Materials (SBOM) generado para un artefacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    /// ID único del SBOM en formato HRN.
    pub id: Hrn,
    
    /// ID del artefacto al que pertenece este SBOM.
    pub artifact_id: Hrn,
    
    /// Formato del SBOM.
    pub format: SbomFormat,
    
    /// Versión de la especificación del SBOM.
    pub spec_version: String,
    
    /// Contenido del SBOM en formato JSON.
    pub content: String,
    
    /// Fecha y hora de creación del SBOM.
    pub created_at: DateTime<Utc>,
    
    /// Herramientas utilizadas para generar el SBOM.
    pub tools: Vec<ToolInformation>,
    
    /// Autores del SBOM.
    pub authors: Vec<ContactInformation>,
    
    /// Número de serie del documento SBOM.
    pub serial_number: String,
    
    /// Nombre del documento SBOM.
    pub document_name: String,
    
    /// Espacio de nombres del documento SBOM.
    pub document_namespace: String,
    
    /// Referencias externas del SBOM.
    pub external_references: Vec<ExternalReference>,
    
    /// Licencia de los datos del SBOM.
    pub data_license: String,
}

/// Formatos soportados para SBOM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SbomFormat {
    /// Formato CycloneDX.
    CycloneDX,
    
    /// Formato SPDX.
    SPDX,
}

/// Información sobre las herramientas utilizadas para generar el SBOM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInformation {
    pub vendor: String,
    pub name: String,
    pub version: String,
}

/// Información de contacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInformation {
    pub name: String,
    pub email: String,
}

/// Referencia externa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalReference {
    pub url: String,
    pub r#type: String,
}