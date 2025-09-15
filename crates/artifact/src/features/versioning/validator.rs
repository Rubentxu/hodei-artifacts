// crates/artifact/src/features/versioning/validator.rs

use crate::features::versioning::dto::{ParsedVersion, VersioningConfig};
use crate::features::versioning::error::VersioningError;
use crate::features::versioning::policy::VersioningPolicy;
use semver::Version;

/// Validador de versiones que parsea y valida cadenas de versión según SemVer 2.0.0
#[derive(Debug, Clone)]
pub struct VersionValidator {
    policy: VersioningPolicy,
}

impl VersionValidator {
    /// Crear un nuevo validador con la política especificada
    pub fn new(policy: VersioningPolicy) -> Self {
        Self { policy }
    }

    /// Crear un nuevo validador con configuración por defecto
    pub fn default() -> Self {
        Self::new(VersioningPolicy::new(VersioningConfig::default()))
    }

    /// Parsear una cadena de versión y devolver una estructura ParsedVersion
    pub fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, VersioningError> {
        // Manejar versiones SNAPSHOT (especialmente para Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remover "-snapshot"
        } else {
            version_str
        };

        // Parsear la versión usando la librería semver
        let version = Version::parse(version_to_parse)
            .map_err(|e| VersioningError::InvalidSemVer(format!("{}: {}", version_str, e)))?;

        // Si se requiere SemVer estricto, verificar que no haya partes no estándar
        if self.policy.is_strict_semver() {
            // La librería semver de Rust ya sigue SemVer 2.0.0, pero podemos hacer
            // verificaciones adicionales si es necesario
        }

        let parsed_version = ParsedVersion {
            original: version_str.to_string(),
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            prerelease: if version.pre.is_empty() {
                None
            } else {
                Some(version.pre.as_str().to_string())
            },
            build_metadata: if version.build.is_empty() {
                None
            } else {
                Some(version.build.as_str().to_string())
            },
            is_snapshot,
        };

        // Validar contra la política
        self.policy.validate_version(&parsed_version)?;

        Ok(parsed_version)
    }

    /// Validar una cadena de versión según las reglas configuradas
    pub fn validate_version(&self, version_str: &str) -> Result<(), VersioningError> {
        let parsed_version = self.parse_version(version_str)?;
        self.policy.validate_version(&parsed_version)
    }

    /// Verificar si una versión ya existe en el repositorio
    /// Esta función requeriría acceso a la capa de datos para verificar
    /// la existencia real de versiones, por ahora solo es un placeholder
    pub fn version_exists(&self, _version_str: &str) -> Result<bool, VersioningError> {
        // En una implementación real, esto consultaría la base de datos
        // para verificar si la versión ya existe
        Ok(false)
    }
}
