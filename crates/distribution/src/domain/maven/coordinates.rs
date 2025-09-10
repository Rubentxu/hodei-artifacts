// crates/distribution/src/domain/maven/coordinates.rs

//! Value Objects para coordenadas Maven - Dominio puro sin dependencias externas

use std::fmt;
use thiserror::Error;

/// Error de validación para coordenadas Maven
#[derive(Debug, Error, Clone, PartialEq)]
pub enum MavenValidationError {
    #[error("Invalid group ID: {0}")]
    InvalidGroupId(String),
    #[error("Invalid artifact ID: {0}")]
    InvalidArtifactId(String),
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
    #[error("Invalid classifier: {0}")]
    InvalidClassifier(String),
    #[error("Invalid extension: {0}")]
    InvalidExtension(String),
}

/// Coordenadas Maven - Value Object inmutable
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MavenCoordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenCoordinates {
    /// Crear nuevas coordenadas Maven con validación
    pub fn new(
        group_id: &str,
        artifact_id: &str,
        version: &str,
    ) -> Result<Self, MavenValidationError> {
        Self::validate_group_id(group_id)?;
        Self::validate_artifact_id(artifact_id)?;
        Self::validate_version(version)?;
        
        Ok(Self {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: "jar".to_string(),
        })
    }
    
    /// Crear coordenadas con clasificador
    pub fn with_classifier(mut self, classifier: &str) -> Result<Self, MavenValidationError> {
        Self::validate_classifier(classifier)?;
        self.classifier = Some(classifier.to_string());
        Ok(self)
    }
    
    /// Crear coordenadas con extensión personalizada
    pub fn with_extension(mut self, extension: &str) -> Result<Self, MavenValidationError> {
        Self::validate_extension(extension)?;
        self.extension = extension.to_string();
        Ok(self)
    }
    
    /// Convertir a path Maven estándar
    /// Ej: com/example/my-app/1.0.0/my-app-1.0.0.jar
    pub fn to_path(&self) -> String {
        let base_path = format!(
            "{}/{}/{}",
            self.group_id.replace('.', "/"),
            self.artifact_id,
            self.version
        );
        
        let filename = if let Some(classifier) = &self.classifier {
            format!("{}-{}-{}.{}", self.artifact_id, self.version, classifier, self.extension)
        } else {
            format!("{}-{}.{}", self.artifact_id, self.version, self.extension)
        };
        
        format!("{}/{}", base_path, filename)
    }
    
    /// Convertir a path de metadata
    /// Ej: com/example/my-app/maven-metadata.xml
    pub fn to_metadata_path(&self) -> String {
        format!(
            "{}/{}/maven-metadata.xml",
            self.group_id.replace('.', "/"),
            self.artifact_id
        )
    }
    
    /// Parsear desde path Maven
    /// Ej: com/example/my-app/1.0.0/my-app-1.0.0.jar
    pub fn from_path(path: &str) -> Result<Self, MavenValidationError> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() < 4 {
            return Err(MavenValidationError::InvalidGroupId("Path too short".to_string()));
        }
        
        // Reconstruir group ID
        let group_parts = &parts[..parts.len() - 3];
        let group_id = group_parts.join(".");
        
        let artifact_id = parts[parts.len() - 3];
        let version = parts[parts.len() - 2];
        let filename = parts[parts.len() - 1];
        
        // Parsear filename
        let expected_prefix = format!("{}-{}", artifact_id, version);
        if !filename.starts_with(&expected_prefix) {
            return Err(MavenValidationError::InvalidArtifactId(
                format!("Filename doesn't start with {}", expected_prefix)
            ));
        }
        
        let remaining = &filename[expected_prefix.len()..];
        let (classifier, extension) = if remaining.starts_with('-') {
            // Tiene clasificador
            let parts: Vec<&str> = remaining[1..].split('.').collect();
            if parts.len() < 2 {
                return Err(MavenValidationError::InvalidExtension("No extension found".to_string()));
            }
            let classifier = parts[..parts.len() - 1].join(".");
            let extension = parts[parts.len() - 1];
            (Some(classifier.as_str()), extension)
        } else if remaining.starts_with('.') {
            // Sin clasificador
            let parts: Vec<&str> = remaining[1..].split('.').collect();
            if parts.is_empty() {
                return Err(MavenValidationError::InvalidExtension("No extension found".to_string()));
            }
            (None, parts[parts.len() - 1])
        } else {
            return Err(MavenValidationError::InvalidArtifactId("Invalid filename format".to_string()));
        };
        
        let mut coordinates = Self::new(group_id, artifact_id, version)?;
        if let Some(classifier) = classifier {
            coordinates = coordinates.with_classifier(classifier)?;
        }
        coordinates = coordinates.with_extension(extension)?;
        
        Ok(coordinates)
    }
    
    /// Validar si es una versión SNAPSHOT
    pub fn is_snapshot(&self) -> bool {
        self.version.ends_with("-SNAPSHOT")
    }
    
    /// Validar si es una versión release
    pub fn is_release(&self) -> bool {
        !self.is_snapshot()
    }
    
    /// Obtener el nombre base del artefacto (sin extensión ni clasificador)
    pub fn base_name(&self) -> String {
        self.artifact_id.clone()
    }
    
    // Métodos de validación - públicos para uso en otros módulos del dominio
    
    pub fn validate_group_id(group_id: &str) -> Result<(), MavenValidationError> {
        if group_id.is_empty() {
            return Err(MavenValidationError::InvalidGroupId("Group ID cannot be empty".to_string()));
        }
        
        if !group_id.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-') {
            return Err(MavenValidationError::InvalidGroupId(
                "Group ID can only contain letters, numbers, '.' and '-'".to_string()
            ));
        }
        
        if group_id.starts_with('.') || group_id.ends_with('.') || 
           group_id.starts_with('-') || group_id.ends_with('-') {
            return Err(MavenValidationError::InvalidGroupId(
                "Group ID cannot start or end with '.' or '-'".to_string()
            ));
        }
        
        if group_id.contains("..") || group_id.contains("--") || 
           group_id.contains(".-") || group_id.contains("-.")) {
            return Err(MavenValidationError::InvalidGroupId(
                "Group ID cannot contain consecutive special characters".to_string()
            ));
        }
        
        Ok(())
    }
    
    pub fn validate_artifact_id(artifact_id: &str) -> Result<(), MavenValidationError> {
        if artifact_id.is_empty() {
            return Err(MavenValidationError::InvalidArtifactId("Artifact ID cannot be empty".to_string()));
        }
        
        if !artifact_id.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(MavenValidationError::InvalidArtifactId(
                "Artifact ID can only contain letters, numbers and '-'".to_string()
            ));
        }
        
        if artifact_id.starts_with('-') || artifact_id.ends_with('-') {
            return Err(MavenValidationError::InvalidArtifactId(
                "Artifact ID cannot start or end with '-'".to_string()
            ));
        }
        
        if artifact_id.contains("--") {
            return Err(MavenValidationError::InvalidArtifactId(
                "Artifact ID cannot contain consecutive hyphens".to_string()
            ));
        }
        
        Ok(())
    }
    
    pub fn validate_version(version: &str) -> Result<(), MavenValidationError> {
        if version.is_empty() {
            return Err(MavenValidationError::InvalidVersion("Version cannot be empty".to_string()));
        }
        
        // Validar formato básico de versión Maven
        if !version.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_') {
            return Err(MavenValidationError::InvalidVersion(
                "Version can only contain letters, numbers, '.', '-' and '_'".to_string()
            ));
        }
        
        Ok(())
    }
    
    pub fn validate_classifier(classifier: &str) -> Result<(), MavenValidationError> {
        if classifier.is_empty() {
            return Err(MavenValidationError::InvalidClassifier("Classifier cannot be empty".to_string()));
        }
        
        if !classifier.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(MavenValidationError::InvalidClassifier(
                "Classifier can only contain letters, numbers and '-'".to_string()
            ));
        }
        
        Ok(())
    }
    
    pub fn validate_extension(extension: &str) -> Result<(), MavenValidationError> {
        if extension.is_empty() {
            return Err(MavenValidationError::InvalidExtension("Extension cannot be empty".to_string()));
        }
        
        if !extension.chars().all(|c| c.is_alphanumeric()) {
            return Err(MavenValidationError::InvalidExtension(
                "Extension can only contain letters and numbers".to_string()
            ));
        }
        
        Ok(())
    }
}

impl fmt::Display for MavenCoordinates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.group_id, self.artifact_id, self.version)?;
        if let Some(classifier) = &self.classifier {
            write!(f, ":{}", classifier)?;
        }
        write!(f, ":{}", self.extension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_coordinates() {
        let coords = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        assert_eq!(coords.group_id, "com.example");
        assert_eq!(coords.artifact_id, "my-app");
        assert_eq!(coords.version, "1.0.0");
        assert_eq!(coords.classifier, None);
        assert_eq!(coords.extension, "jar");
    }
    
    #[test]
    fn test_with_classifier() {
        let coords = MavenCoordinates::new("com.example", "my-app", "1.0.0")
            .unwrap()
            .with_classifier("sources")
            .unwrap();
        
        assert_eq!(coords.classifier, Some("sources".to_string()));
    }
    
    #[test]
    fn test_with_extension() {
        let coords = MavenCoordinates::new("com.example", "my-app", "1.0.0")
            .unwrap()
            .with_extension("war")
            .unwrap();
        
        assert_eq!(coords.extension, "war");
    }
    
    #[test]
    fn test_to_path() {
        let coords = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        assert_eq!(coords.to_path(), "com/example/my-app/1.0.0/my-app-1.0.0.jar");
        
        let coords_with_classifier = MavenCoordinates::new("com.example", "my-app", "1.0.0")
            .unwrap()
            .with_classifier("sources")
            .unwrap();
        assert_eq!(coords_with_classifier.to_path(), "com/example/my-app/1.0.0/my-app-1.0.0-sources.jar");
    }
    
    #[test]
    fn test_from_path() {
        let path = "com/example/my-app/1.0.0/my-app-1.0.0.jar";
        let coords = MavenCoordinates::from_path(path).unwrap();
        
        assert_eq!(coords.group_id, "com.example");
        assert_eq!(coords.artifact_id, "my-app");
        assert_eq!(coords.version, "1.0.0");
        assert_eq!(coords.extension, "jar");
    }
    
    #[test]
    fn test_is_snapshot() {
        let snapshot = MavenCoordinates::new("com.example", "my-app", "1.0.0-SNAPSHOT").unwrap();
        assert!(snapshot.is_snapshot());
        
        let release = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        assert!(release.is_release());
    }
    
    #[test]
    fn test_invalid_group_id() {
        let result = MavenCoordinates::new("", "my-app", "1.0.0");
        assert!(result.is_err());
        
        let result = MavenCoordinates::new("com..example", "my-app", "1.0.0");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_invalid_artifact_id() {
        let result = MavenCoordinates::new("com.example", "", "1.0.0");
        assert!(result.is_err());
        
        let result = MavenCoordinates::new("com.example", "my--app", "1.0.0");
        assert!(result.is_err());
    }
}