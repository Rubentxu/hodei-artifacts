// crates/supply-chain/src/domain/slsa.rs

use shared::models::ContentHash;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Estructura que representa el contenido de un predicado de procedencia SLSA v1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaProvenancePredicate {
    pub builder: SlsaBuilder,
    pub recipe: SlsaRecipe,
    pub invocation: SlsaInvocation,
    pub materials: Vec<SlsaMaterial>,
    pub metadata: SlsaMetadata,
}

/// Identifica el constructor que generó el artefacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaBuilder {
    pub id: String, // URI identificando el constructor
}

/// Describe cómo se construyó el artefacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaRecipe {
    #[serde(rename = "type")]
    pub recipe_type: String, // URI
    #[serde(rename = "entryPoint")]
    pub entry_point: String,
    pub arguments: serde_json::Value,
}

/// Describe la ejecución específica del proceso de construcción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaInvocation {
    #[serde(rename = "configSource")]
    pub config_source: SlsaConfigSource,
    pub parameters: serde_json::Value,
    pub environment: serde_json::Value,
}

/// Materiales (ej. código fuente) utilizados en la construcción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaMaterial {
    pub uri: String,
    pub digest: HashMap<String, String>, // ej. {"sha1": "...", "sha256": "..."}
}

/// Metadatos adicionales sobre la construcción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaMetadata {
    pub build_started_on: Option<String>,
    pub build_finished_on: Option<String>,
    pub completeness: SlsaCompleteness,
    pub reproducible: bool,
}

/// Describe la completitud de los metadatos de construcción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaCompleteness {
    pub parameters: bool,
    pub environment: bool,
    pub materials: bool,
}

/// Describe la fuente de configuración usada para la construcción.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaConfigSource {
    pub uri: String,
    pub digest: HashMap<String, String>,
    pub entry_point: String,
}