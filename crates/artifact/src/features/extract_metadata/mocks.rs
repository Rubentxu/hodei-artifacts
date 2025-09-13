// crates/artifact/src/features/extract_metadata/mocks.rs

use async_trait::async_trait;
use bytes::Bytes;
use shared::hrn::Hrn;
use crate::domain::package_version::{PackageMetadata, ArtifactDependency};
use crate::domain::events::ArtifactMetadataEnriched;
use super::{
    error::MetadataError,
    ports::{LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe, ArtifactContentReader, MetadataEventPublisher},
};

/// Mock implementation for package metadata repository
#[derive(Default)]
pub struct MockPackageMetadataRepository {
    stored_metadata: std::sync::Mutex<std::collections::HashMap<String, (PackageMetadata, Vec<ArtifactDependency>)>>,
    should_fail: std::sync::atomic::AtomicBool,
}

impl MockPackageMetadataRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_should_fail(&self, should_fail: bool) {
        self.should_fail.store(should_fail, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn add_mock_metadata(&self, hrn: &str, metadata: PackageMetadata, dependencies: Vec<ArtifactDependency>) {
        self.stored_metadata.lock().unwrap().insert(hrn.to_string(), (metadata, dependencies));
    }

    pub fn get_stored_metadata(&self, hrn: &str) -> Option<(PackageMetadata, Vec<ArtifactDependency>)> {
        self.stored_metadata.lock().unwrap().get(hrn).cloned()
    }
}

#[async_trait]
impl LeqNtT4aDY9oM1G5gAWWvB8B39iUobThhe for MockPackageMetadataRepository {
    async fn update_package_metadata(
        &self,
        hrn: &Hrn,
        metadata: PackageMetadata,
        dependencies: Vec<ArtifactDependency>,
    ) -> Result<(), MetadataError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(MetadataError::RepositoryError("Mock repository error".to_string()));
        }

        let hrn_str = hrn.to_string();
        self.stored_metadata.lock().unwrap().insert(hrn_str, (metadata, dependencies));
        Ok(())
    }
}

/// Mock implementation for artifact content reader
#[derive(Default)]
pub struct MockArtifactContentReader {
    mock_content: std::sync::Mutex<std::collections::HashMap<String, Bytes>>,
    should_fail: std::sync::atomic::AtomicBool,
}

impl MockArtifactContentReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_should_fail(&self, should_fail: bool) {
        self.should_fail.store(should_fail, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn add_mock_content(&self, path: &str, content: Bytes) {
        self.mock_content.lock().unwrap().insert(path.to_string(), content);
    }

    pub fn add_mock_maven_pom(&self, path: &str) {
        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>
    
    <groupId>com.example</groupId>
    <artifactId>test-artifact</artifactId>
    <version>1.0.0</version>
    <description>Test Maven artifact</description>
    
    <licenses>
        <license>
            <name>Apache License 2.0</name>
        </license>
    </licenses>
    
    <dependencies>
        <dependency>
            <groupId>org.springframework</groupId>
            <artifactId>spring-core</artifactId>
            <version>5.3.21</version>
            <scope>compile</scope>
        </dependency>
    </dependencies>
</project>"#;
        self.add_mock_content(path, Bytes::from(pom_content));
    }

    pub fn add_mock_npm_package(&self, path: &str) {
        let package_content = r#"{
    "name": "test-package",
    "version": "1.0.0",
    "description": "Test NPM package",
    "license": "MIT",
    "dependencies": {
        "lodash": "^4.17.21",
        "express": "^4.18.2"
    },
    "devDependencies": {
        "jest": "^29.0.0"
    }
}"#;
        self.add_mock_content(path, Bytes::from(package_content));
    }
}

#[async_trait]
impl ArtifactContentReader for MockArtifactContentReader {
    async fn read_artifact_content(&self, storage_path: &str) -> Result<Bytes, MetadataError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(MetadataError::StorageError("Mock storage error".to_string()));
        }

        self.mock_content.lock().unwrap()
            .get(storage_path)
            .cloned()
            .ok_or_else(|| MetadataError::StorageError(format!("Content not found for path: {}", storage_path)))
    }
}

/// Mock implementation for metadata event publisher
#[derive(Default)]
pub struct MockMetadataEventPublisher {
    published_events: std::sync::Mutex<Vec<ArtifactMetadataEnriched>>,
    should_fail: std::sync::atomic::AtomicBool,
}

impl MockMetadataEventPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_should_fail(&self, should_fail: bool) {
        self.should_fail.store(should_fail, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn get_published_events(&self) -> Vec<ArtifactMetadataEnriched> {
        self.published_events.lock().unwrap().clone()
    }

    pub fn clear_events(&self) {
        self.published_events.lock().unwrap().clear();
    }

    pub fn get_event_count(&self) -> usize {
        self.published_events.lock().unwrap().len()
    }
}

#[async_trait]
impl MetadataEventPublisher for MockMetadataEventPublisher {
    async fn publish_metadata_enriched(&self, event: ArtifactMetadataEnriched) -> Result<(), MetadataError> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(MetadataError::EventError("Mock event error".to_string()));
        }

        self.published_events.lock().unwrap().push(event);
        Ok(())
    }
}

/// Utility for creating test HRNs
pub fn create_test_hrn(package_name: &str, version: &str) -> Hrn {
    Hrn::new(&format!("hrn:artifact:example/{}:{}", package_name, version))
        .expect("Failed to create test HRN")
}

/// Utility for creating test package metadata
pub fn create_test_metadata(description: &str) -> PackageMetadata {
    PackageMetadata {
        description: Some(description.to_string()),
        licenses: vec!["MIT".to_string(), "Apache-2.0".to_string()],
        authors: vec!["Test Author".to_string()],
        project_url: Some("https://github.com/test/project".to_string()),
        repository_url: Some("https://github.com/test/project.git".to_string()),
        last_downloaded_at: None,
        download_count: 0,
        custom_properties: std::collections::HashMap::new(),
    }
}

/// Utility for creating test dependencies
pub fn create_test_dependency(name: &str, version: &str) -> ArtifactDependency {
    ArtifactDependency {
        coordinates: crate::domain::package_version::PackageCoordinates {
            namespace: Some("com.example".to_string()),
            name: name.to_string(),
            version: version.to_string(),
            qualifiers: std::collections::HashMap::new(),
        },
        scope: "compile".to_string(),
        version_constraint: version.to_string(),
        is_optional: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_repository() {
        let repository = MockPackageMetadataRepository::new();
        let hrn = create_test_hrn("test-package", "1.0.0");
        let metadata = create_test_metadata("Test package");
        let dependencies = vec![create_test_dependency("dep1", "1.0.0")];

        // Store metadata
        repository.update_package_metadata(&hrn, metadata.clone(), dependencies.clone()).await.unwrap();

        // Retrieve metadata
        let stored = repository.get_stored_metadata(&hrn.to_string());
        assert!(stored.is_some());
        let (stored_meta, stored_deps) = stored.unwrap();
        assert_eq!(stored_meta.description, metadata.description);
        assert_eq!(stored_deps.len(), dependencies.len());
    }

    #[tokio::test]
    async fn test_mock_content_reader() {
        let reader = MockArtifactContentReader::new();
        let test_path = "/test/path";
        let test_content = Bytes::from("test content");

        reader.add_mock_content(test_path, test_content.clone());

        // Read content
        let content = reader.read_artifact_content(test_path).await.unwrap();
        assert_eq!(content, test_content);

        // Test non-existent path
        let result = reader.read_artifact_content("/non/existent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_event_publisher() {
        let publisher = MockMetadataEventPublisher::new();
        let hrn = create_test_hrn("test-package", "1.0.0");
        let metadata = create_test_metadata("Test package");

        let event = ArtifactMetadataEnriched {
            package_version_hrn: hrn,
            extracted_metadata: metadata,
            at: time::OffsetDateTime::now_utc(),
        };

        // Publish event
        publisher.publish_metadata_enriched(event.clone()).await.unwrap();

        // Check event was published
        let events = publisher.get_published_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].package_version_hrn.to_string(), event.package_version_hrn.to_string());
    }

    #[tokio::test]
    async fn test_mock_error_conditions() {
        let repository = MockPackageMetadataRepository::new();
        let reader = MockArtifactContentReader::new();
        let publisher = MockMetadataEventPublisher::new();

        // Test repository error
        repository.set_should_fail(true);
        let hrn = create_test_hrn("test", "1.0.0");
        let result = repository.update_package_metadata(&hrn, create_test_metadata("test"), vec![]).await;
        assert!(result.is_err());

        // Test reader error
        reader.set_should_fail(true);
        let result = reader.read_artifact_content("/test/path").await;
        assert!(result.is_err());

        // Test publisher error
        publisher.set_should_fail(true);
        let event = ArtifactMetadataEnriched {
            package_version_hrn: hrn,
            extracted_metadata: create_test_metadata("test"),
            at: time::OffsetDateTime::now_utc(),
        };
        let result = publisher.publish_metadata_enriched(event).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_pom_and_package_content() {
        let reader = MockArtifactContentReader::new();
        let pom_path = "/test/pom.xml";
        let package_path = "/test/package.json";

        reader.add_mock_maven_pom(pom_path);
        reader.add_mock_npm_package(package_path);

        // Test Maven POM content
        let pom_content = reader.read_artifact_content(pom_path).await.unwrap();
        let pom_str = String::from_utf8_lossy(&pom_content);
        assert!(pom_str.contains("com.example"));
        assert!(pom_str.contains("test-artifact"));

        // Test NPM package content
        let package_content = reader.read_artifact_content(package_path).await.unwrap();
        let package_str = String::from_utf8_lossy(&package_content);
        assert!(package_str.contains("test-package"));
        assert!(package_str.contains("lodash"));
    }
}