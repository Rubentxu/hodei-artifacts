use std::sync::Arc;
use bytes::Bytes;
use async_trait::async_trait;
use tracing::debug;
use quick_xml::de::from_str;
use serde_json;
use std::collections::HashMap;
use crate::features::upload_artifact::ports::{ArtifactStorage, ArtifactRepository};
use crate::domain::events::ArtifactMetadataEnriched;
use crate::domain::package_version::{PackageMetadata, ArtifactDependency};
use shared::hrn::Hrn;
use super::{
    ports::{LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe, ArtifactContentReader, MetadataEventPublisher},
    error::MetadataError,
    dto::{ParsedMavenMetadata, MavenDependency, ParsedNpmMetadata, NpmDependency},
};

/// Adapter for reading artifact content from storage
pub struct StorageArtifactContentReader {
    // storage: Arc<dyn ArtifactStorage>,
}

impl StorageArtifactContentReader {
    pub fn new(_storage: Arc<dyn ArtifactStorage>) -> Self {
        Self {}
    }
}

#[async_trait]
impl ArtifactContentReader for StorageArtifactContentReader {
    async fn read_artifact_content(&self, storage_path: &str) -> Result<Bytes, MetadataError> {
        debug!("Reading artifact content from storage path: {}", storage_path);
        // In a real implementation, we would need to download the content from storage
        // For now, we'll return an error as this requires a different approach
        Err(MetadataError::StorageError("Not implemented: Reading artifact content from storage path requires downloading the file".to_string()))
    }
}

/// Adapter for updating package metadata in repository
pub struct RepositoryMetadataUpdater {
    // repository: Arc<dyn ArtifactRepository>,
}

impl RepositoryMetadataUpdater {
    pub fn new(_repository: Arc<dyn ArtifactRepository>) -> Self {
        Self {}
    }
}

#[async_trait]
impl LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe for RepositoryMetadataUpdater {
    async fn update_package_metadata(
        &self,
        _hrn: &Hrn,
        _metadata: PackageMetadata,
        _dependencies: Vec<ArtifactDependency>,
    ) -> Result<(), MetadataError> {
        debug!("Updating package metadata");
        // In a real implementation, we would need to update the existing package version
        // with the new metadata and dependencies
        Err(MetadataError::RepositoryError("Not implemented: Updating package metadata in repository".to_string()))
    }
}

/// Adapter for publishing metadata enrichment events
pub struct EventBusMetadataPublisher {
    // In a real implementation, this would hold a reference to the event publisher
}

impl EventBusMetadataPublisher {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl MetadataEventPublisher for EventBusMetadataPublisher {
    async fn publish_metadata_enriched(&self, event: ArtifactMetadataEnriched) -> Result<(), MetadataError> {
        debug!("Publishing metadata enriched event for package: {}", event.package_version_hrn);
        // In a real implementation, we would publish the event to the event bus
        Err(MetadataError::EventError("Not implemented: Publishing metadata enriched event".to_string()))
    }
}

// ===== Maven Parsing Adapter =====

/// Root element of a Maven POM file
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename = "project")]
struct MavenProject {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
    
    #[serde(rename = "artifactId")]
    artifact_id: String,
    
    version: Option<String>,
    
    description: Option<String>,
    
    #[serde(rename = "licenses")]
    licenses: Option<MavenLicenses>,
    
    #[serde(rename = "dependencies")]
    dependencies: Option<MavenDependencies>,
    
    #[serde(rename = "parent")]
    parent: Option<MavenParent>,
}

/// Parent POM reference
#[derive(Debug, Clone, serde::Deserialize)]
struct MavenParent {
    #[serde(rename = "groupId")]
    group_id: String,
    
    #[serde(rename = "artifactId")]
    artifact_id: String,
    
    version: String,
}

/// Container for licenses in POM
#[derive(Debug, Clone, serde::Deserialize)]
struct MavenLicenses {
    #[serde(rename = "license")]
    licenses: Vec<MavenLicense>,
}

/// License information
#[derive(Debug, Clone, serde::Deserialize)]
struct MavenLicense {
    name: String,
    url: Option<String>,
}

/// Container for dependencies in POM
#[derive(Debug, Clone, serde::Deserialize)]
struct MavenDependencies {
    #[serde(rename = "dependency")]
    dependencies: Vec<MavenDependencyElement>,
}

/// Single dependency element
#[derive(Debug, Clone, serde::Deserialize)]
struct MavenDependencyElement {
    #[serde(rename = "groupId")]
    group_id: String,
    
    #[serde(rename = "artifactId")]
    artifact_id: String,
    
    version: Option<String>,
    
    scope: Option<String>,
    
    #[serde(rename = "optional")]
    optional: Option<String>,
}

/// Adapter for parsing Maven POM files
pub struct MavenMetadataAdapter;

impl MavenMetadataAdapter {
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

// ===== NPM Parsing Adapter =====

/// Structure representing a package.json file
#[derive(Debug, Clone, serde::Deserialize)]
struct NpmPackage {
    name: String,
    version: String,
    description: Option<String>,
    
    #[serde(rename = "license")]
    license_single: Option<String>,
    
    #[serde(rename = "licenses")]
    licenses_multiple: Option<Vec<NpmLicense>>,
    
    dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<HashMap<String, String>>,
}

/// License structure in package.json
#[derive(Debug, Clone, serde::Deserialize)]
struct NpmLicense {
    #[serde(rename = "type")]
    type_: Option<String>,
    name: Option<String>,
    url: Option<String>,
}

/// Adapter for parsing NPM package.json files
pub struct NpmMetadataAdapter;

impl NpmMetadataAdapter {
    pub fn new() -> Self {
        Self
    }
    
    /// Parse NPM package.json content and extract metadata
    pub fn parse(&self, json_content: &str) -> Result<ParsedNpmMetadata, MetadataError> {
        // Parse JSON content
        let package: NpmPackage = serde_json::from_str(json_content)
            .map_err(|e| MetadataError::ParseError(format!("Failed to parse package.json: {}", e)))?;
        
        // Extract licenses
        let licenses = self.extract_licenses(&package);
        
        // Extract dependencies
        let mut dependencies = Vec::new();
        
        // Regular dependencies
        if let Some(deps) = &package.dependencies {
            for (name, version) in deps {
                dependencies.push(NpmDependency {
                    name: name.clone(),
                    version: version.clone(),
                    is_dev_dependency: false,
                });
            }
        }
        
        // Dev dependencies
        if let Some(dev_deps) = &package.dev_dependencies {
            for (name, version) in dev_deps {
                dependencies.push(NpmDependency {
                    name: name.clone(),
                    version: version.clone(),
                    is_dev_dependency: true,
                });
            }
        }
        
        Ok(ParsedNpmMetadata {
            name: package.name,
            version: package.version,
            description: package.description,
            licenses,
            dependencies,
        })
    }
    
    /// Extract license information from package.json
    fn extract_licenses(&self, package: &NpmPackage) -> Vec<String> {
        let mut licenses = Vec::new();
        
        // Single license field
        if let Some(single_license) = &package.license_single {
            licenses.push(single_license.clone());
        }
        
        // Multiple licenses field
        if let Some(multiple_licenses) = &package.licenses_multiple {
            for license in multiple_licenses {
                if let Some(name) = &license.name {
                    licenses.push(name.clone());
                } else if let Some(type_) = &license.type_ {
                    licenses.push(type_.clone());
                }
            }
        }
        
        licenses
    }
}

// ===== Factory for metadata parsers =====

/// Factory for creating appropriate metadata adapters based on artifact type
pub struct MetadataAdapterFactory;

impl MetadataAdapterFactory {
    /// Create the appropriate adapter for the given artifact type
    pub fn create_adapter(artifact_type: &str) -> Result<Box<dyn MetadataAdapter>, MetadataError> {
        match artifact_type.to_lowercase().as_str() {
            "maven" | "pom" => Ok(Box::new(MavenMetadataAdapter::new())),
            "npm" | "package.json" => Ok(Box::new(NpmMetadataAdapter::new())),
            _ => Err(MetadataError::UnsupportedArtifactType(artifact_type.to_string())),
        }
    }
}

/// Trait for metadata adapters
pub trait MetadataAdapter: Send + Sync {
    fn parse(&self, content: &str) -> Result<(ParsedMavenMetadata, Vec<MavenDependency>), MetadataError> {
        // Default implementation returns error
        Err(MetadataError::ParseError("This adapter does not support Maven parsing".to_string()))
    }
    
    fn parse_npm(&self, content: &str) -> Result<(ParsedNpmMetadata, Vec<NpmDependency>), MetadataError> {
        // Default implementation returns error
        Err(MetadataError::ParseError("This adapter does not support NPM parsing".to_string()))
    }
}

// Implement the trait for Maven adapter
impl MetadataAdapter for MavenMetadataAdapter {
    fn parse(&self, content: &str) -> Result<(ParsedMavenMetadata, Vec<MavenDependency>), MetadataError> {
        let metadata = self.parse(content)?;
        Ok((metadata.clone(), metadata.dependencies.clone()))
    }
}

// Implement the trait for NPM adapter
impl MetadataAdapter for NpmMetadataAdapter {
    fn parse_npm(&self, content: &str) -> Result<(ParsedNpmMetadata, Vec<NpmDependency>), MetadataError> {
        let metadata = self.parse(content)?;
        Ok((metadata.clone(), metadata.dependencies.clone()))
    }
}
