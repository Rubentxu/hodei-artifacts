// crates/supply-chain/src/domain/sbom.rs

use shared::models::ContentHash;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

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