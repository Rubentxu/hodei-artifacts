// crates/distribution/src/domain/npm/npm_paths.rs

use crate::domain::error::{FormatError, DistributionResult};
use shared::models::PackageCoordinates;
use std::path::Path;

/// Información extraída de un path npm
#[derive(Debug, Clone)]
pub struct NpmPathInfo {
    pub package_name: String,
    pub version: Option<String>,
    pub filename: Option<String>,
    pub is_metadata: bool,
    pub is_tarball: bool,
    pub is_version_metadata: bool,
    pub coordinates: Option<PackageCoordinates>,
}

/// Parser de paths npm siguiendo la especificación oficial
pub struct NpmPathParser;

impl NpmPathParser {
    pub fn new() -> Self {
        Self
    }

    /// Parsea un path npm y extrae información relevante
    /// 
    /// Formatos soportados:
    /// - /{package}                    # Metadata del paquete
    /// - /{package}/{version}          # Metadata específica de versión
    /// - /{package}/-/{filename}       # Archivo (tarball, etc.)
    /// - /-/user/{user}/package/{package}/version/{version} # Publicación
    pub fn parse_path(&self, path: &str) -> DistributionResult<NpmPathInfo> {
        let clean_path = path.trim_start_matches('/');
        let components: Vec<&str> = clean_path.split('/').collect();
        
        if components.is_empty() {
            return Err(FormatError::NpmError("Empty path".to_string()).into());
        }

        // Detectar tipo de path basado en la estructura
        match components.as_slice() {
            // Metadata del paquete: /{package}
            [package] => {
                self.parse_package_metadata(package)
            }
            // Metadata de versión: /{package}/{version}
            [package, version] => {
                self.parse_version_metadata(package, version)
            }
            // Archivo: /{package}/-/{filename}
            [package, "-", filename] => {
                self.parse_tarball(package, filename)
            }
            // Publicación: /-/user/{user}/package/{package}/version/{version}
            ["-", "user", user, "package", package, "version", version] => {
                self.parse_publish_path(user, package, version)
            }
            // Dist-tags: /-/package/{package}/dist-tags
            ["-", "package", package, "dist-tags"] => {
                self.parse_dist_tags(package)
            }
            // Dist-tag específico: /-/package/{package}/dist-tags/{tag}
            ["-", "package", package, "dist-tags", tag] => {
                self.parse_dist_tag(package, tag)
            }
            _ => {
                Err(FormatError::NpmError(format!("Unsupported npm path format: {}", path)).into())
            }
        }
    }

    /// Parsea metadata de paquete: /{package}
    fn parse_package_metadata(&self, package: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: None,
            filename: None,
            is_metadata: true,
            is_tarball: false,
            is_version_metadata: false,
            coordinates: None,
        })
    }

    /// Parsea metadata de versión: /{package}/{version}
    fn parse_version_metadata(&self, package: &str, version: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;
        self.validate_version(version)?;

        let coordinates = PackageCoordinates {
            group: package.to_string(), // npm usa el nombre como grupo
            name: package.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: Some("json".to_string()),
        };

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: Some(version.to_string()),
            filename: None,
            is_metadata: true,
            is_tarball: false,
            is_version_metadata: true,
            coordinates: Some(coordinates),
        })
    }

    /// Parsea tarball: /{package}/-/{filename}
    fn parse_tarball(&self, package: &str, filename: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;

        // Extraer versión del filename (formato: package-version.tgz)
        let version = self.extract_version_from_filename(package, filename)?;

        let coordinates = PackageCoordinates {
            group: package.to_string(),
            name: package.to_string(),
            version: version.clone(),
            classifier: None,
            extension: Some("tgz".to_string()),
        };

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: Some(version),
            filename: Some(filename.to_string()),
            is_metadata: false,
            is_tarball: true,
            is_version_metadata: false,
            coordinates: Some(coordinates),
        })
    }

    /// Parsea publicación: /-/user/{user}/package/{package}/version/{version}
    fn parse_publish_path(&self, _user: &str, package: &str, version: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;
        self.validate_version(version)?;

        let coordinates = PackageCoordinates {
            group: package.to_string(),
            name: package.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: Some("json".to_string()),
        };

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: Some(version.to_string()),
            filename: None,
            is_metadata: true,
            is_tarball: false,
            is_version_metadata: true,
            coordinates: Some(coordinates),
        })
    }

    /// Parsea dist-tags: /-/package/{package}/dist-tags
    fn parse_dist_tags(&self, package: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: None,
            filename: None,
            is_metadata: true,
            is_tarball: false,
            is_version_metadata: false,
            coordinates: None,
        })
    }

    /// Parsea dist-tag específico: /-/package/{package}/dist-tags/{tag}
    fn parse_dist_tag(&self, package: &str, _tag: &str) -> DistributionResult<NpmPathInfo> {
        self.validate_package_name(package)?;

        Ok(NpmPathInfo {
            package_name: package.to_string(),
            version: None,
            filename: None,
            is_metadata: true,
            is_tarball: false,
            is_version_metadata: false,
            coordinates: None,
        })
    }

    /// Valida el nombre del paquete npm
    fn validate_package_name(&self, name: &str) -> DistributionResult<()> {
        if name.is_empty() {
            return Err(FormatError::NpmError("Package name cannot be empty".to_string()).into());
        }

        if name.len() > 214 {
            return Err(FormatError::NpmError("Package name too long (max 214 characters)".to_string()).into());
        }

        // npm package name rules
        // - Debe ser lowercase
        // - Puede contener letras, números, guiones, guiones bajos, puntos
        // - No puede empezar con punto o guión
        // - No puede terminar con punto
        if name.chars().any(|c| c.is_uppercase()) {
            return Err(FormatError::NpmError("Package name must be lowercase".to_string()).into());
        }

        if name.starts_with('.') || name.starts_with('-') {
            return Err(FormatError::NpmError("Package name cannot start with '.' or '-'".to_string()).into());
        }

        if name.ends_with('.') {
            return Err(FormatError::NpmError("Package name cannot end with '.'".to_string()).into());
        }

        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(FormatError::NpmError("Package name can only contain letters, numbers, '-', '_', and '.'".to_string()).into());
        }

        Ok(())
    }

    /// Valida la versión npm
    fn validate_version(&self, version: &str) -> DistributionResult<()> {
        if version.is_empty() {
            return Err(FormatError::NpmError("Version cannot be empty".to_string()).into());
        }

        // Versión npm debe seguir semver
        // Formato básico: major.minor.patch[-prerelease][+buildmetadata]
        let parts: Vec<&str> = version.split(&['-', '+'][..]).collect();
        let version_part = parts[0];
        
        let numbers: Vec<&str> = version_part.split('.').collect();
        if numbers.len() != 3 {
            return Err(FormatError::NpmError("Version must follow semver format: major.minor.patch".to_string()).into());
        }

        for num in numbers {
            if num.parse::<u64>().is_err() {
                return Err(FormatError::NpmError(format!("Invalid version number: {}", num)).into());
            }
        }

        Ok(())
    }

    /// Extrae la versión del filename del tarball
    fn extract_version_from_filename(&self, package: &str, filename: &str) -> DistributionResult<String> {
        let expected_prefix = format!("{}-", package);
        let expected_suffix = ".tgz";

        if !filename.starts_with(&expected_prefix) {
            return Err(FormatError::NpmError(format!(
                "Tarball filename must start with '{}', got: {}", 
                expected_prefix, filename
            )).into());
        }

        if !filename.ends_with(expected_suffix) {
            return Err(FormatError::NpmError(format!(
                "Tarball filename must end with '{}', got: {}", 
                expected_suffix, filename
            )).into());
        }

        let version_start = expected_prefix.len();
        let version_end = filename.len() - expected_suffix.len();
        
        if version_start >= version_end {
            return Err(FormatError::NpmError("Invalid tarball filename format".to_string()).into());
        }

        let version = &filename[version_start..version_end];
        self.validate_version(version)?;
        
        Ok(version.to_string())
    }

    /// Construye un path npm para metadata
    pub fn build_metadata_path(&self, package_name: &str) -> String {
        format!("/{}", package_name)
    }

    /// Construye un path npm para metadata de versión
    pub fn build_version_metadata_path(&self, package_name: &str, version: &str) -> String {
        format!("/{}/{}", package_name, version)
    }

    /// Construye un path npm para tarball
    pub fn build_tarball_path(&self, package_name: &str, version: &str) -> String {
        format!("/{}-/{}-{}.tgz", package_name, package_name, version)
    }

    /// Construye un path npm para dist-tags
    pub fn build_dist_tags_path(&self, package_name: &str) -> String {
        format!("/-/package/{}/dist-tags", package_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_metadata() {
        let parser = NpmPathParser::new();
        
        let path = "/my-package";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.package_name, "my-package");
        assert_eq!(info.version, None);
        assert_eq!(info.filename, None);
        assert!(info.is_metadata);
        assert!(!info.is_tarball);
        assert!(!info.is_version_metadata);
        assert!(info.coordinates.is_none());
    }

    #[test]
    fn test_parse_version_metadata() {
        let parser = NpmPathParser::new();
        
        let path = "/my-package/1.2.3";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.package_name, "my-package");
        assert_eq!(info.version, Some("1.2.3".to_string()));
        assert_eq!(info.filename, None);
        assert!(info.is_metadata);
        assert!(!info.is_tarball);
        assert!(info.is_version_metadata);
        assert!(info.coordinates.is_some());
    }

    #[test]
    fn test_parse_tarball() {
        let parser = NpmPathParser::new();
        
        let path = "/my-package-/my-package-1.2.3.tgz";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.package_name, "my-package");
        assert_eq!(info.version, Some("1.2.3".to_string()));
        assert_eq!(info.filename, Some("my-package-1.2.3.tgz".to_string()));
        assert!(!info.is_metadata);
        assert!(info.is_tarball);
        assert!(!info.is_version_metadata);
        assert!(info.coordinates.is_some());
    }

    #[test]
    fn test_parse_publish_path() {
        let parser = NpmPathParser::new();
        
        let path = "/-/user/john/package/my-package/version/1.2.3";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.package_name, "my-package");
        assert_eq!(info.version, Some("1.2.3".to_string()));
        assert_eq!(info.filename, None);
        assert!(info.is_metadata);
        assert!(!info.is_tarball);
        assert!(info.is_version_metadata);
        assert!(info.coordinates.is_some());
    }

    #[test]
    fn test_parse_dist_tags() {
        let parser = NpmPathParser::new();
        
        let path = "/-/package/my-package/dist-tags";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.package_name, "my-package");
        assert_eq!(info.version, None);
        assert_eq!(info.filename, None);
        assert!(info.is_metadata);
        assert!(!info.is_tarball);
        assert!(!info.is_version_metadata);
        assert!(info.coordinates.is_none());
    }

    #[test]
    fn test_validate_package_name() {
        let parser = NpmPathParser::new();
        
        // Válidos
        assert!(parser.validate_package_name("my-package").is_ok());
        assert!(parser.validate_package_name("my_package").is_ok());
        assert!(parser.validate_package_name("my.package").is_ok());
        assert!(parser.validate_package_name("my123package").is_ok());
        
        // Inválidos
        assert!(parser.validate_package_name("").is_err());
        assert!(parser.validate_package_name(".package").is_err());
        assert!(parser.validate_package_name("-package").is_err());
        assert!(parser.validate_package_name("package.").is_err());
        assert!(parser.validate_package_name("MyPackage").is_err());
        assert!(parser.validate_package_name("my/package").is_err());
    }

    #[test]
    fn test_validate_version() {
        let parser = NpmPathParser::new();
        
        // Válidos
        assert!(parser.validate_version("1.2.3").is_ok());
        assert!(parser.validate_version("0.0.1").is_ok());
        assert!(parser.validate_version("10.20.30").is_ok());
        
        // Inválidos
        assert!(parser.validate_version("").is_err());
        assert!(parser.validate_version("1").is_err());
        assert!(parser.validate_version("1.2").is_err());
        assert!(parser.validate_version("1.2.3.4").is_err());
        assert!(parser.validate_version("a.b.c").is_err());
    }

    #[test]
    fn test_extract_version_from_filename() {
        let parser = NpmPathParser::new();
        
        let version = parser.extract_version_from_filename("my-package", "my-package-1.2.3.tgz").unwrap();
        assert_eq!(version, "1.2.3");
        
        // Inválidos
        assert!(parser.extract_version_from_filename("my-package", "wrong-package-1.2.3.tgz").is_err());
        assert!(parser.extract_version_from_filename("my-package", "my-package-1.2.3.tar.gz").is_err());
    }

    #[test]
    fn test_build_paths() {
        let parser = NpmPathParser::new();
        
        assert_eq!(parser.build_metadata_path("my-package"), "/my-package");
        assert_eq!(parser.build_version_metadata_path("my-package", "1.2.3"), "/my-package/1.2.3");
        assert_eq!(parser.build_tarball_path("my-package", "1.2.3"), "/my-package-/my-package-1.2.3.tgz");
        assert_eq!(parser.build_dist_tags_path("my-package"), "/-/package/my-package/dist-tags");
    }

    #[test]
    fn test_invalid_paths() {
        let parser = NpmPathParser::new();
        
        // Path vacío
        assert!(parser.parse_path("").is_err());
        
        // Path demasiado corto
        assert!(parser.parse_path("/").is_err());
        
        // Formato no soportado
        assert!(parser.parse_path("/unknown/format/path").is_err());
    }
}