// crates/supply-chain/src/domain/sbom.rs

use shared::models::ContentHash;
use shared::hrn::Hrn;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

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

/// Estructura que representa el contenido de un predicado SBOM, serializable a/desde JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomPredicate {
    pub spec_version: String,
    pub serial_number: Option<String>,
    pub components: Vec<SbomComponent>,
    pub relationships: Vec<SbomRelationship>,
    pub tools: Vec<Tool>,
}

/// Representa un componente de software dentro de un SBOM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomComponent {
    /// Identificador PURL (Package URL) del componente.
    pub purl: String,
    pub component_type: ComponentType,
    pub name: String,
    pub version: String,
    pub supplier: Option<String>,
    pub hashes: Vec<ContentHash>,
    pub licenses: Vec<String>, // SPDX license identifiers
}

/// Representa una relación entre componentes en el SBOM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomRelationship {
    /// El PURL del componente de origen.
    pub source_purl: String,
    /// El PURL del componente de destino.
    pub target_purl: String,
    /// El tipo de relación (ej. "DEPENDS_ON", "CONTAINS").
    pub relationship_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub vendor: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentType { Library, Application, Framework, OperatingSystem, Device, File }

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