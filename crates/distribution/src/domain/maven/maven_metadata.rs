// crates/distribution/src/domain/maven/maven_metadata.rs

use quick_xml::events::{Event, BytesStart, BytesEnd};
use quick_xml::Writer;
use std::io::Cursor;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use crate::domain::error::{FormatError, DistributionResult};
use shared::hrn::RepositoryId;
use shared::models::PackageCoordinates;

/// Estructura de maven-metadata.xml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenMetadata {
    pub group_id: String,
    pub artifact_id: String,
    pub versioning: MavenVersioning,
}

/// Información de versionado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenVersioning {
    pub latest: Option<String>,
    pub release: Option<String>,
    pub versions: Vec<String>,
    pub last_updated: String, // Formato: YYYYMMDDHHMMSS
}

/// Generador de maven-metadata.xml
pub struct MavenMetadataGenerator;

impl MavenMetadataGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Genera maven-metadata.xml para un artefacto específico
    pub fn generate_metadata(
        &self,
        group_id: &str,
        artifact_id: &str,
        versions: Vec<String>,
    ) -> DistributionResult<String> {
        // Determinar latest y release
        let latest = versions.last().cloned();
        let release = self.find_release_version(&versions);

        // Generar timestamp
        let now = OffsetDateTime::now_utc();
        let last_updated = now.format(&Rfc3339)
            .map_err(|e| FormatError::MavenError(format!("Failed to format timestamp: {}", e)))?
            .replace("-", "")
            .replace(":", "")
            .replace("T", "")
            .chars()
            .take(14) // YYYYMMDDHHMMSS
            .collect::<String>();

        let metadata = MavenMetadata {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
            versioning: MavenVersioning {
                latest,
                release,
                versions,
                last_updated,
            },
        };

        self.serialize_to_xml(&metadata)
    }

    /// Genera maven-metadata.xml para un grupo completo
    pub fn generate_group_metadata(
        &self,
        group_id: &str,
        artifacts: Vec<(String, Vec<String>)>, // (artifact_id, versions)
    ) -> DistributionResult<String> {
        // Para metadata de grupo, usamos una estructura simplificada
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<metadata>\n");
        xml.push_str(&format!("  <groupId>{}</groupId>\n", group_id));
        
        if !artifacts.is_empty() {
            xml.push_str("  <plugins>\n");
            for (artifact_id, versions) in artifacts {
                xml.push_str(&format!("    <plugin>\n"));
                xml.push_str(&format!("      <name>{}</name>\n", artifact_id));
                xml.push_str(&format!("      <prefix>{}</prefix>\n", artifact_id));
                xml.push_str(&format!("      <artifactId>{}</artifactId>\n", artifact_id));
                xml.push_str("    </plugin>\n");
            }
            xml.push_str("  </plugins>\n");
        }
        
        xml.push_str("</metadata>\n");
        Ok(xml)
    }

    /// Serializa metadata a XML
    fn serialize_to_xml(&self, metadata: &MavenMetadata) -> DistributionResult<String> {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        
        // XML declaration
        writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        ))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Root metadata element
        let mut metadata_elem = BytesStart::new("metadata");
        metadata_elem.push_attribute(("modelVersion", "1.1.0"));
        writer.write_event(Event::Start(metadata_elem)).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Group ID
        writer.write_event(Event::Start(BytesStart::new("groupId"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(&metadata.group_id)))
            .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::End(BytesEnd::new("groupId"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Artifact ID
        writer.write_event(Event::Start(BytesStart::new("artifactId"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(&metadata.artifact_id)))
            .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::End(BytesEnd::new("artifactId"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Versioning section
        writer.write_event(Event::Start(BytesStart::new("versioning"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Latest version
        if let Some(latest) = &metadata.versioning.latest {
            writer.write_event(Event::Start(BytesStart::new("latest"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(latest)))
                .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            writer.write_event(Event::End(BytesEnd::new("latest"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        }

        // Release version
        if let Some(release) = &metadata.versioning.release {
            writer.write_event(Event::Start(BytesStart::new("release"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(release)))
                .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            writer.write_event(Event::End(BytesEnd::new("release"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        }

        // Versions list
        if !metadata.versioning.versions.is_empty() {
            writer.write_event(Event::Start(BytesStart::new("versions"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            
            for version in &metadata.versioning.versions {
                writer.write_event(Event::Start(BytesStart::new("version"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
                writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(version)))
                    .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
                writer.write_event(Event::End(BytesEnd::new("version"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
            }
            
            writer.write_event(Event::End(BytesEnd::new("versions"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        }

        // Last updated
        writer.write_event(Event::Start(BytesStart::new("lastUpdated"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::Text(quick_xml::events::BytesText::from_plain_str(&metadata.versioning.last_updated)))
            .map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;
        writer.write_event(Event::End(BytesEnd::new("lastUpdated"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Close versioning section
        writer.write_event(Event::End(BytesEnd::new("versioning"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Close metadata element
        writer.write_event(Event::End(BytesEnd::new("metadata"))).map_err(|e| FormatError::MavenError(format!("XML write error: {}", e)))?;

        // Convert to string
        let xml_bytes = writer.into_inner().into_inner();
        String::from_utf8(xml_bytes)
            .map_err(|e| FormatError::MavenError(format!("UTF-8 conversion error: {}", e)))
    }

    /// Determina la versión de release (no SNAPSHOT)
    fn find_release_version(&self, versions: &[String]) -> Option<String> {
        versions.iter()
            .rev() // Empezar por la más reciente
            .find(|v| !v.contains("-SNAPSHOT"))
            .cloned()
    }

    /// Parsea maven-metadata.xml existente
    pub fn parse_metadata(&self, xml_content: &str) -> DistributionResult<MavenMetadata> {
        // Implementación simplificada - en producción usar quick-xml para parsear
        // Por ahora, generamos uno nuevo basado en lo que tenemos
        Err(FormatError::MavenError("Parse not implemented yet".to_string()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_simple_metadata() {
        let generator = MavenMetadataGenerator::new();
        
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "1.2.0".to_string(),
        ];
        
        let xml = generator.generate_metadata("com.example", "my-app", versions).unwrap();
        
        // Verificar que contiene los elementos esperados
        assert!(xml.contains("<groupId>com.example</groupId>"));
        assert!(xml.contains("<artifactId>my-app</artifactId>"));
        assert!(xml.contains("<version>1.0.0</version>"));
        assert!(xml.contains("<version>1.1.0</version>"));
        assert!(xml.contains("<version>1.2.0</version>"));
        assert!(xml.contains("<latest>1.2.0</latest>"));
        assert!(xml.contains("<release>1.2.0</release>"));
        assert!(xml.contains("<lastUpdated>"));
    }

    #[test]
    fn test_generate_metadata_with_snapshots() {
        let generator = MavenMetadataGenerator::new();
        
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0-SNAPSHOT".to_string(),
            "1.1.0".to_string(),
        ];
        
        let xml = generator.generate_metadata("com.example", "my-app", versions).unwrap();
        
        // La release debe ser la última versión que no sea SNAPSHOT
        assert!(xml.contains("<release>1.1.0</release>"));
        assert!(xml.contains("<latest>1.1.0</latest>"));
    }

    #[test]
    fn test_generate_empty_metadata() {
        let generator = MavenMetadataGenerator::new();
        
        let xml = generator.generate_metadata("com.example", "my-app", vec![]).unwrap();
        
        assert!(xml.contains("<groupId>com.example</groupId>"));
        assert!(xml.contains("<artifactId>my-app</artifactId>"));
        // No debe tener latest/release si no hay versiones
        assert!(!xml.contains("<latest>"));
        assert!(!xml.contains("<release>"));
    }
}