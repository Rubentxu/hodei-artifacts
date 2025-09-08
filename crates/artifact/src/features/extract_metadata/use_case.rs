use std::sync::Arc;
use tracing::{info, error, warn};
use time::OffsetDateTime;
use crate::domain::package_version::{PackageMetadata, ArtifactDependency, PackageCoordinates};
use crate::domain::events::ArtifactMetadataEnriched;
use super::{
    dto::{ExtractMetadataCommand, MetadataExtractionResult},
    error::MetadataError,
    ports::{LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe, ArtifactContentReader, MetadataEventPublisher},
    maven_parser::MavenParser,
    npm_parser::NpmParser,
};

/// Use case for extracting metadata from uploaded artifacts
pub struct ExtractMetadataUseCase {
    repository: Arc<dyn LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe>,
    content_reader: Arc<dyn ArtifactContentReader>,
    event_publisher: Arc<dyn MetadataEventPublisher>,
}

impl ExtractMetadataUseCase {
    pub fn new(
        repository: Arc<dyn LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe>,
        content_reader: Arc<dyn ArtifactContentReader>,
        event_publisher: Arc<dyn MetadataEventPublisher>,
    ) -> Self {
        Self {
            repository,
            content_reader,
            event_publisher,
        }
    }
    
    /// Execute the metadata extraction process
    pub async fn execute(&self, command: ExtractMetadataCommand) -> Result<MetadataExtractionResult, MetadataError> {
        info!("Extracting metadata for package version: {}", command.package_version_hrn);
        
        // Read artifact content
        let artifact_content = self.content_reader
            .read_artifact_content(&command.artifact_storage_path)
            .await
            .map_err(|e| {
                error!("Failed to read artifact content: {}", e);
                MetadataError::StorageError(e.to_string())
            })?;
        
        // Convert bytes to string for parsing
        let content_str = String::from_utf8_lossy(&artifact_content);
        
        // Parse metadata based on artifact type
        let (metadata, dependencies) = match command.artifact_type.as_str() {
            "maven" => {
                info!("Parsing Maven POM file");
                self.parse_maven_metadata(&content_str)?
            },
            "npm" => {
                info!("Parsing NPM package.json file");
                self.parse_npm_metadata(&content_str)?
            },
            _ => {
                warn!("Unsupported artifact type: {}", command.artifact_type);
                return Err(MetadataError::UnsupportedArtifactType(command.artifact_type));
            }
        };
        
        // Update package metadata in repository
        self.repository
            .update_package_metadata(
                &command.package_version_hrn,
                metadata.clone(),
                dependencies.clone(),
            )
            .await
            .map_err(|e| {
                error!("Failed to update package metadata: {}", e);
                MetadataError::RepositoryError(e.to_string())
            })?;
        
        // Publish metadata enriched event
        let event = ArtifactMetadataEnriched {
            package_version_hrn: command.package_version_hrn.clone(),
            extracted_metadata: metadata.clone(),
            at: OffsetDateTime::now_utc(),
        };
        
        self.event_publisher
            .publish_metadata_enriched(event)
            .await
            .map_err(|e| {
                error!("Failed to publish metadata enriched event: {}", e);
                MetadataError::EventError(e.to_string())
            })?;
        
        info!("Successfully extracted and published metadata for package: {}", command.package_version_hrn);
        
        Ok(MetadataExtractionResult {
            package_version_hrn: command.package_version_hrn,
            extracted_metadata: metadata,
            extracted_dependencies: dependencies,
        })
    }
    
    /// Parse Maven POM metadata
    fn parse_maven_metadata(&self, content: &str) -> Result<(PackageMetadata, Vec<ArtifactDependency>), MetadataError> {
        let parser = MavenParser::new();
        let parsed_metadata = parser.parse(content)?;
        
        let metadata = PackageMetadata {
            description: parsed_metadata.description,
            licenses: parsed_metadata.licenses,
            authors: Vec::new(), // Authors not typically in POM
            project_url: None,  // Project URL not typically in POM
            repository_url: None, // Repository URL not typically in POM
            last_downloaded_at: None,
            download_count: 0,
            custom_properties: std::collections::HashMap::new(),
        };
        
        let dependencies = parsed_metadata.dependencies
            .into_iter()
            .map(|dep| ArtifactDependency {
                coordinates: PackageCoordinates {
                    namespace: Some(dep.group_id),
                    name: dep.artifact_id,
                    version: dep.version,
                    qualifiers: std::collections::HashMap::new(),
                },
                scope: dep.scope,
                version_constraint: String::new(), // Not extracted from POM
                is_optional: false, // Not typically in POM
            })
            .collect();
        
        Ok((metadata, dependencies))
    }
    
    /// Parse NPM package.json metadata
    fn parse_npm_metadata(&self, content: &str) -> Result<(PackageMetadata, Vec<ArtifactDependency>), MetadataError> {
        let parser = NpmParser::new();
        let parsed_metadata = parser.parse(content)?;
        
        let metadata = PackageMetadata {
            description: parsed_metadata.description,
            licenses: parsed_metadata.licenses,
            authors: Vec::new(), // Authors not typically in package.json
            project_url: None,   // Project URL not typically in package.json
            repository_url: None, // Repository URL not typically in package.json
            last_downloaded_at: None,
            download_count: 0,
            custom_properties: std::collections::HashMap::new(),
        };
        
        let dependencies = parsed_metadata.dependencies
            .into_iter()
            .map(|dep| ArtifactDependency {
                coordinates: PackageCoordinates {
                    namespace: None, // NPM packages don't have namespaces in the same way
                    name: dep.name,
                    version: dep.version,
                    qualifiers: std::collections::HashMap::new(),
                },
                scope: if dep.is_dev_dependency { "dev".to_string() } else { "dependencies".to_string() },
                version_constraint: String::new(), // Not extracted from package.json
                is_optional: false,
            })
            .collect();
        
        Ok((metadata, dependencies))
    }
}