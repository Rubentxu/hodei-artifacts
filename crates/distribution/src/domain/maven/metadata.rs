// crates/distribution/src/domain/maven/metadata.rs

//! Metadata Maven - Entidades y value objects para maven-metadata.xml

use std::collections::HashMap;
use time::{Date, OffsetDateTime};
use serde::{Serialize, Deserialize};

/// Versión Maven con información adicional
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MavenVersion {
    pub version: String,
    pub last_updated: OffsetDateTime,
}

impl MavenVersion {
    pub fn new(version: String) -> Self {
        Self {
            version,
            last_updated: OffsetDateTime::now_utc(),
        }
    }
    
    pub fn with_timestamp(mut self, timestamp: OffsetDateTime) -> Self {
        self.last_updated = timestamp;
        self
    }
}

/// Metadata de un artefacto Maven
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MavenMetadata {
    pub group_id: String,
    pub artifact_id: String,
    pub versions: Vec<MavenVersion>,
    pub last_updated: OffsetDateTime,
}

impl MavenMetadata {
    pub fn new(group_id: String, artifact_id: String) -> Self {
        Self {
            group_id,
            artifact_id,
            versions: Vec::new(),
            last_updated: OffsetDateTime::now_utc(),
        }
    }
    
    /// Agregar una versión nueva
    pub fn add_version(&mut self, version: MavenVersion) {
        if !self.versions.contains(&version) {
            self.versions.push(version);
            self.versions.sort();
            self.update_timestamp();
        }
    }
    
    /// Agregar múltiples versiones
    pub fn add_versions(&mut self, versions: Vec<MavenVersion>) {
        for version in versions {
            self.add_version(version);
        }
    }
    
    /// Obtener la versión más reciente
    pub fn get_latest_version(&self) -> Option<&MavenVersion> {
        self.versions.last()
    }
    
    /// Obtener la versión más reciente que no sea SNAPSHOT
    pub fn get_latest_release(&self) -> Option<&MavenVersion> {
        self.versions.iter()
            .filter(|v| !v.version.ends_with("-SNAPSHOT"))
            .last()
    }
    
    /// Verificar si existe una versión específica
    pub fn has_version(&self, version: &str) -> bool {
        self.versions.iter().any(|v| v.version == version)
    }
    
    /// Obtener todas las versiones como strings
    pub fn get_version_strings(&self) -> Vec<String> {
        self.versions.iter().map(|v| v.version.clone()).collect()
    }
    
    /// Actualizar timestamp
    fn update_timestamp(&mut self) {
        self.last_updated = OffsetDateTime::now_utc();
    }
    
    /// Generar XML maven-metadata.xml
    pub fn to_xml(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata modelVersion="1.1.0">
  <groupId>{}</groupId>
  <artifactId>{}</artifactId>
  <versioning>
    <latest>{}</latest>
    <release>{}</release>
    <versions>
{}
    </versions>
    <lastUpdated>{}</lastUpdated>
  </versioning>
</metadata>"#,
            self.group_id,
            self.artifact_id,
            self.get_latest_version().map(|v| &v.version).unwrap_or(&"".to_string()),
            self.get_latest_release().map(|v| &v.version).unwrap_or(&"".to_string()),
            self.versions.iter()
                .map(|v| format!("      <version>{}</version>", v.version))
                .collect::<Vec<_>>()
                .join("\n"),
            self.last_updated.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
        )
    }
    
    /// Parsear desde XML
    pub fn from_xml(xml: &str) -> Result<Self, MavenMetadataError> {
        // Implementación simplificada - en producción usaría un parser XML robusto
        let mut metadata = Self::new(
            Self::extract_tag(xml, "groupId")?,
            Self::extract_tag(xml, "artifactId")?
        );
        
        if let Ok(last_updated) = Self::extract_tag(xml, "lastUpdated") {
            if let Ok(timestamp) = OffsetDateTime::parse(&last_updated, &time::format_description::well_known::Rfc3339) {
                metadata.last_updated = timestamp;
            }
        }
        
        // Extraer versiones
        let versions_content = Self::extract_tag_content(xml, "versions")?;
        for line in versions_content.lines() {
            if let Some(version) = Self::extract_version_from_line(line) {
                metadata.add_version(MavenVersion::new(version));
            }
        }
        
        Ok(metadata)
    }
    
    fn extract_tag(xml: &str, tag: &str) -> Result<String, MavenMetadataError> {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);
        
        let start = xml.find(&start_tag)
            .ok_or_else(|| MavenMetadataError::ParseError(format!("Start tag {} not found", tag)))?;
        let end = xml.find(&end_tag)
            .ok_or_else(|| MavenMetadataError::ParseError(format!("End tag {} not found", tag)))?;
        
        Ok(xml[start + start_tag.len()..end].trim().to_string())
    }
    
    fn extract_tag_content(xml: &str, tag: &str) -> Result<String, MavenMetadataError> {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);
        
        let start = xml.find(&start_tag)
            .ok_or_else(|| MavenMetadataError::ParseError(format!("Start tag {} not found", tag)))?;
        let end = xml.find(&end_tag)
            .ok_or_else(|| MavenMetadataError::ParseError(format!("End tag {} not found", tag)))?;
        
        Ok(xml[start + start_tag.len()..end].trim().to_string())
    }
    
    fn extract_version_from_line(line: &str) -> Option<String> {
        if line.trim().starts_with("<version>") && line.trim().ends_with("</version>") {
            let start = line.find("<version>")? + "<version>".len();
            let end = line.find("</version>")?;
            Some(line[start..end].trim().to_string())
        } else {
            None
        }
    }
}

/// Error al parsear metadata
#[derive(Debug, thiserror::Error)]
pub enum MavenMetadataError {
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

/// Información de versionado para un grupo de artefactos
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MavenGroupMetadata {
    pub group_id: String,
    pub artifacts: HashMap<String, MavenMetadata>,
}

impl MavenGroupMetadata {
    pub fn new(group_id: String) -> Self {
        Self {
            group_id,
            artifacts: HashMap::new(),
        }
    }
    
    pub fn add_artifact_metadata(&mut self, metadata: MavenMetadata) {
        self.artifacts.insert(metadata.artifact_id.clone(), metadata);
    }
    
    pub fn get_artifact_metadata(&self, artifact_id: &str) -> Option<&MavenMetadata> {
        self.artifacts.get(artifact_id)
    }
    
    pub fn list_artifacts(&self) -> Vec<&String> {
        self.artifacts.keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_maven_version_ordering() {
        let v1 = MavenVersion::new("1.0.0".to_string());
        let v2 = MavenVersion::new("2.0.0".to_string());
        let v3 = MavenVersion::new("1.0.0-SNAPSHOT".to_string());
        
        assert!(v1 < v2);
        assert!(v3 < v1); // SNAPSHOT viene antes
    }
    
    #[test]
    fn test_maven_metadata() {
        let mut metadata = MavenMetadata::new("com.example".to_string(), "my-app".to_string());
        
        metadata.add_version(MavenVersion::new("1.0.0".to_string()));
        metadata.add_version(MavenVersion::new("1.1.0".to_string()));
        metadata.add_version(MavenVersion::new("2.0.0".to_string()));
        
        assert_eq!(metadata.versions.len(), 3);
        assert_eq!(metadata.get_latest_version().unwrap().version, "2.0.0");
        assert_eq!(metadata.get_latest_release().unwrap().version, "2.0.0");
    }
    
    #[test]
    fn test_maven_metadata_with_snapshot() {
        let mut metadata = MavenMetadata::new("com.example".to_string(), "my-app".to_string());
        
        metadata.add_version(MavenVersion::new("1.0.0".to_string()));
        metadata.add_version(MavenVersion::new("1.1.0-SNAPSHOT".to_string()));
        metadata.add_version(MavenVersion::new("2.0.0-SNAPSHOT".to_string()));
        
        assert_eq!(metadata.get_latest_version().unwrap().version, "2.0.0-SNAPSHOT");
        assert_eq!(metadata.get_latest_release().unwrap().version, "1.0.0");
    }
    
    #[test]
    fn test_to_xml() {
        let mut metadata = MavenMetadata::new("com.example".to_string(), "my-app".to_string());
        metadata.add_version(MavenVersion::new("1.0.0".to_string()));
        metadata.add_version(MavenVersion::new("1.1.0".to_string()));
        
        let xml = metadata.to_xml();
        assert!(xml.contains("<groupId>com.example</groupId>"));
        assert!(xml.contains("<artifactId>my-app</artifactId>"));
        assert!(xml.contains("<version>1.0.0</version>"));
        assert!(xml.contains("<version>1.1.0</version>"));
    }
}