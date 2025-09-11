// crates/artifact/src/features/versioning/dto.rs

use serde::{Deserialize, Serialize};

/// Configuración para el versionado de artefactos en un repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersioningConfig {
    /// Si es true, solo se permiten versiones que siguen SemVer estricto
    pub strict_semver: bool,
    
    /// Si es true, se permite solo una versión SNAPSHOT por combinación major.minor
    pub allow_only_one_snapshot_per_major_minor: bool,
    
    /// Lista de tags pre-release permitidos (vacío significa todos permitidos)
    pub allowed_prerelease_tags: Vec<String>,
    
    /// Si es true, se rechazan versiones con metadata de build
    pub reject_build_metadata: bool,
}

impl Default for VersioningConfig {
    fn default() -> Self {
        Self {
            strict_semver: false,
            allow_only_one_snapshot_per_major_minor: false,
            allowed_prerelease_tags: vec![],
            reject_build_metadata: false,
        }
    }
}

/// Información detallada sobre una versión parseada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedVersion {
    /// La versión original como string
    pub original: String,
    
    /// Parte major de la versión
    pub major: u64,
    
    /// Parte minor de la versión
    pub minor: u64,
    
    /// Parte patch de la versión
    pub patch: u64,
    
    /// Tags pre-release si existen
    pub prerelease: Option<String>,
    
    /// Metadata de build si existe
    pub build_metadata: Option<String>,
    
    /// Si es una versión SNAPSHOT (especialmente para Maven)
    pub is_snapshot: bool,
}