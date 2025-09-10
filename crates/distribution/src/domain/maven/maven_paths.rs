// crates/distribution/src/domain/maven/maven_paths.rs

use crate::domain::error::{FormatError, DistributionResult};
use shared::models::PackageCoordinates;
use std::path::Path;

/// Información extraída de un path Maven
#[derive(Debug, Clone)]
pub struct MavenPathInfo {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub filename: String,
    pub extension: String,
    pub is_metadata: bool,
    pub coordinates: PackageCoordinates,
}

/// Parser de paths Maven siguiendo la especificación oficial
pub struct MavenPathParser;

impl MavenPathParser {
    pub fn new() -> Self {
        Self
    }

    /// Parsea un path Maven y extrae información relevante
    /// 
    /// Formatos soportados:
    /// - /group/id/artifact-id/version/artifact-id-version.jar
    /// - /group/id/artifact-id/version/artifact-id-version.pom
    /// - /group/id/artifact-id/maven-metadata.xml
    /// - /group/id/artifact-id/version/maven-metadata.xml
    pub fn parse_path(&self, path: &str) -> DistributionResult<MavenPathInfo> {
        let clean_path = path.trim_start_matches('/');
        let components: Vec<&str> = clean_path.split('/').collect();
        
        if components.is_empty() {
            return Err(FormatError::InvalidPath("Empty path".to_string()).into());
        }

        // Detectar si es metadata
        let is_metadata = components.last().map(|&last| last == "maven-metadata.xml").unwrap_or(false);
        
        if is_metadata {
            self.parse_metadata_path(&components)
        } else {
            self.parse_artifact_path(&components)
        }
    }

    /// Parsea paths de artefactos (JAR, POM, etc.)
    fn parse_artifact_path(&self, components: &[&str]) -> DistributionResult<MavenPathInfo> {
        if components.len() < 4 {
            return Err(FormatError::InvalidPath(
                format!("Artifact path must have at least 4 components, got {}", components.len())
            ).into());
        }

        let len = components.len();
        let filename = components[len - 1];
        let version = components[len - 2];
        let artifact_id = components[len - 3];
        
        // Construir group_id de los componentes restantes
        let group_id = if len > 4 {
            components[0..len - 3].join(".")
        } else {
            components[0].to_string()
        };

        // Extraer extensión
        let extension = Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Validar que el filename sigue el patrón Maven
        let expected_prefix = format!("{}-{}", artifact_id, version);
        if !filename.starts_with(&expected_prefix) {
            return Err(FormatError::InvalidPath(format!(
                "Filename '{}' does not follow Maven naming convention '{}-*'",
                filename, expected_prefix
            )).into());
        }

        let coordinates = PackageCoordinates {
            group: group_id.clone(),
            name: artifact_id.clone(),
            version: version.clone(),
            classifier: None, // Podríamos extraerlo del filename si existe
            extension: if extension.is_empty() { None } else { Some(extension.clone()) },
        };

        Ok(MavenPathInfo {
            group_id,
            artifact_id: artifact_id.to_string(),
            version: version.to_string(),
            filename: filename.to_string(),
            extension,
            is_metadata: false,
            coordinates,
        })
    }

    /// Parsea paths de metadata
    fn parse_metadata_path(&self, components: &[&str]) -> DistributionResult<MavenPathInfo> {
        if components.len() < 3 {
            return Err(FormatError::InvalidPath(
                format!("Metadata path must have at least 3 components, got {}", components.len())
            ).into());
        }

        let len = components.len();
        let is_version_metadata = len >= 4 && components[len - 1] == "maven-metadata.xml";
        
        let (artifact_id, version) = if is_version_metadata {
            (components[len - 2], components[len - 3])
        } else {
            (components[len - 2], "LATEST")
        };

        // Construir group_id
        let group_id = if len > 3 {
            let group_components = if is_version_metadata {
                &components[0..len - 3]
            } else {
                &components[0..len - 2]
            };
            group_components.join(".")
        } else {
            components[0].to_string()
        };

        let coordinates = PackageCoordinates {
            group: group_id.clone(),
            name: artifact_id.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: Some("xml".to_string()),
        };

        Ok(MavenPathInfo {
            group_id,
            artifact_id: artifact_id.to_string(),
            version: version.to_string(),
            filename: "maven-metadata.xml".to_string(),
            extension: "xml".to_string(),
            is_metadata: true,
            coordinates,
        })
    }

    /// Construye un path Maven a partir de coordenadas
    pub fn build_path(&self, coordinates: &PackageCoordinates, filename: &str) -> String {
        let group_path = coordinates.group.replace('.', "/");
        format!(
            "/{}/{}/{}/{}",
            group_path,
            coordinates.name,
            coordinates.version,
            filename
        )
    }

    /// Construye el path para maven-metadata.xml
    pub fn build_metadata_path(&self, coordinates: &PackageCoordinates, include_version: bool) -> String {
        let group_path = coordinates.group.replace('.', "/");
        let base = format!("/{}/{}", group_path, coordinates.name);
        
        if include_version {
            format!("{}/{}/maven-metadata.xml", base, coordinates.version)
        } else {
            format!("{}/maven-metadata.xml", base)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_artifact_path() {
        let parser = MavenPathParser::new();
        
        // Test JAR path
        let path = "/com/example/my-app/1.0.0/my-app-1.0.0.jar";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.group_id, "com.example");
        assert_eq!(info.artifact_id, "my-app");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.filename, "my-app-1.0.0.jar");
        assert_eq!(info.extension, "jar");
        assert!(!info.is_metadata);
    }

    #[test]
    fn test_parse_pom_path() {
        let parser = MavenPathParser::new();
        
        let path = "/org/springframework/boot/spring-boot-starter/2.7.0/spring-boot-starter-2.7.0.pom";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.group_id, "org.springframework.boot");
        assert_eq!(info.artifact_id, "spring-boot-starter");
        assert_eq!(info.version, "2.7.0");
        assert_eq!(info.filename, "spring-boot-starter-2.7.0.pom");
        assert_eq!(info.extension, "pom");
        assert!(!info.is_metadata);
    }

    #[test]
    fn test_parse_metadata_path() {
        let parser = MavenPathParser::new();
        
        let path = "/com/example/my-app/maven-metadata.xml";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.group_id, "com.example");
        assert_eq!(info.artifact_id, "my-app");
        assert_eq!(info.version, "LATEST");
        assert_eq!(info.filename, "maven-metadata.xml");
        assert_eq!(info.extension, "xml");
        assert!(info.is_metadata);
    }

    #[test]
    fn test_parse_version_metadata_path() {
        let parser = MavenPathParser::new();
        
        let path = "/com/example/my-app/1.0.0/maven-metadata.xml";
        let info = parser.parse_path(path).unwrap();
        
        assert_eq!(info.group_id, "com.example");
        assert_eq!(info.artifact_id, "my-app");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.filename, "maven-metadata.xml");
        assert_eq!(info.extension, "xml");
        assert!(info.is_metadata);
    }

    #[test]
    fn test_build_path() {
        let parser = MavenPathParser::new();
        
        let coordinates = PackageCoordinates {
            group: "com.example".to_string(),
            name: "my-app".to_string(),
            version: "1.0.0".to_string(),
            classifier: None,
            extension: Some("jar".to_string()),
        };
        
        let path = parser.build_path(&coordinates, "my-app-1.0.0.jar");
        assert_eq!(path, "/com/example/my-app/1.0.0/my-app-1.0.0.jar");
    }

    #[test]
    fn test_build_metadata_path() {
        let parser = MavenPathParser::new();
        
        let coordinates = PackageCoordinates {
            group: "com.example".to_string(),
            name: "my-app".to_string(),
            version: "1.0.0".to_string(),
            classifier: None,
            extension: Some("xml".to_string()),
        };
        
        let path = parser.build_metadata_path(&coordinates, false);
        assert_eq!(path, "/com/example/my-app/maven-metadata.xml");
        
        let version_path = parser.build_metadata_path(&coordinates, true);
        assert_eq!(version_path, "/com/example/my-app/1.0.0/maven-metadata.xml");
    }

    #[test]
    fn test_invalid_path() {
        let parser = MavenPathParser::new();
        
        // Path demasiado corto
        let result = parser.parse_path("/com/example");
        assert!(result.is_err());
        
        // Filename inválido
        let result = parser.parse_path("/com/example/my-app/1.0.0/wrong-name.jar");
        assert!(result.is_err());
    }
}