use quick_xml::de::from_str;
use serde::Deserialize;
use super::dto::{ParsedMavenMetadata, MavenDependency};
use super::error::MetadataError;

/// Root element of a Maven POM file
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "project")]
pub struct MavenProject {
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
    
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    
    pub version: Option<String>,
    
    pub description: Option<String>,
    
    #[serde(rename = "licenses")]
    pub licenses: Option<MavenLicenses>,
    
    #[serde(rename = "dependencies")]
    pub dependencies: Option<MavenDependencies>,
    
    #[serde(rename = "parent")]
    pub parent: Option<MavenParent>,
}

/// Parent POM reference
#[derive(Debug, Clone, Deserialize)]
pub struct MavenParent {
    #[serde(rename = "groupId")]
    pub group_id: String,
    
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    
    pub version: String,
}

/// Container for licenses in POM
#[derive(Debug, Clone, Deserialize)]
pub struct MavenLicenses {
    #[serde(rename = "license")]
    pub licenses: Vec<MavenLicense>,
}

/// License information
#[derive(Debug, Clone, Deserialize)]
pub struct MavenLicense {
    pub name: String,
    pub url: Option<String>,
}

/// Container for dependencies in POM
#[derive(Debug, Clone, Deserialize)]
pub struct MavenDependencies {
    #[serde(rename = "dependency")]
    pub dependencies: Vec<MavenDependencyElement>,
}

/// Single dependency element
#[derive(Debug, Clone, Deserialize)]
pub struct MavenDependencyElement {
    #[serde(rename = "groupId")]
    pub group_id: String,
    
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    
    pub version: Option<String>,
    
    pub scope: Option<String>,
    
    #[serde(rename = "optional")]
    pub optional: Option<String>,
}

/// Parser for Maven POM files
pub struct MavenParser;

impl MavenParser {
    pub fn new() -> Self {
        Self
    }
    
    /// Parse Maven POM XML content and extract metadata
    pub fn parse(&self, xml_content: &str) -> Result<ParsedMavenMetadata, MetadataError> {
        // Parse XML content
        let project: MavenProject = from_str(xml_content)
            .map_err(|e| MetadataError::ParseError(format!("Failed to parse Maven POM: {}", e)))?;
        
        // Extract basic information
        let group_id = project.group_id
            .or_else(|| project.parent.as_ref().map(|p| p.group_id.clone()))
            .unwrap_or_default();
            
        let version = project.version
            .or_else(|| project.parent.as_ref().map(|p| p.version.clone()))
            .unwrap_or_default();
        
        // Extract licenses
        let licenses = project.licenses
            .as_ref()
            .map(|l| l.licenses.iter().map(|license| license.name.clone()).collect())
            .unwrap_or_else(Vec::new);
        
        // Extract dependencies
        let dependencies = project.dependencies
            .as_ref()
            .map(|deps| {
                deps.dependencies.iter().map(|dep| {
                    MavenDependency {
                        group_id: dep.group_id.clone(),
                        artifact_id: dep.artifact_id.clone(),
                        version: dep.version.clone().unwrap_or_default(),
                        scope: dep.scope.clone().unwrap_or_else(|| "compile".to_string()),
                    }
                }).collect()
            })
            .unwrap_or_else(Vec::new);
        
        Ok(ParsedMavenMetadata {
            group_id,
            artifact_id: project.artifact_id,
            version,
            description: project.description,
            licenses,
            dependencies,
        })
    }
}