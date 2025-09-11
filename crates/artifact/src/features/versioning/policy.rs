// crates/artifact/src/features/versioning/policy.rs

use crate::features::versioning::dto::{VersioningConfig, ParsedVersion};
use crate::features::versioning::error::VersioningError;

/// Política de versionado que define las reglas para un repositorio
#[derive(Debug, Clone)]
pub struct VersioningPolicy {
    config: VersioningConfig,
}

impl VersioningPolicy {
    /// Crear una nueva política de versionado con la configuración especificada
    pub fn new(config: VersioningConfig) -> Self {
        Self { config }
    }
    
    /// Obtener la configuración de la política
    pub fn config(&self) -> &VersioningConfig {
        &self.config
    }
    
    /// Validar si una versión cumple con las políticas definidas
    pub fn validate_version(&self, parsed_version: &ParsedVersion) -> Result<(), VersioningError> {
        // Validar política de SNAPSHOT
        if parsed_version.is_snapshot && self.config.allow_only_one_snapshot_per_major_minor {
            // Esta validación requeriría consultar la base de datos para verificar
            // si ya existe una versión SNAPSHOT para la misma combinación major.minor
            // Por ahora dejamos este chequeo para una implementación posterior
        }
        
        // Validar tags pre-release permitidos
        if let Some(ref prerelease) = parsed_version.prerelease {
            if !self.config.allowed_prerelease_tags.is_empty() 
                && !self.config.allowed_prerelease_tags.iter().any(|tag| prerelease.contains(tag)) {
                return Err(VersioningError::PrereleaseTagNotAllowed(prerelease.clone()));
            }
        }
        
        // Validar metadata de build
        if parsed_version.build_metadata.is_some() && self.config.reject_build_metadata {
            return Err(VersioningError::BuildMetadataNotAllowed(
                parsed_version.build_metadata.clone().unwrap_or_default()
            ));
        }
        
        Ok(())
    }
    
    /// Verificar si una versión es estrictamente SemVer
    pub fn is_strict_semver(&self) -> bool {
        self.config.strict_semver
    }
}