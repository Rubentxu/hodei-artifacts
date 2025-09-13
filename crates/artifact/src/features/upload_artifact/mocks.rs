use async_trait::async_trait;
use bytes::Bytes;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::domain::events::ArtifactEvent;
use crate::domain::package_version::{PackageVersion, PackageMetadata, ArtifactDependency};
use crate::domain::physical_artifact::PhysicalArtifact;
use super::ports::{
    ArtifactRepository, ArtifactStorage, EventPublisher, ArtifactValidator, VersionValidator,
    ParsedVersion, PortResult,
};
use super::dto::UploadArtifactCommand;
use super::error::UploadArtifactError;

#[derive(Default, Debug)]
pub struct MockArtifactRepository {
    pub physical_artifacts: Arc<Mutex<Vec<PhysicalArtifact>>>,
    pub package_versions: Arc<Mutex<Vec<PackageVersion>>>,
    pub should_fail_save_physical_artifact: Arc<Mutex<bool>>,
}

impl MockArtifactRepository {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn count_physical_artifacts(&self) -> usize {
        self.physical_artifacts.lock().unwrap().len()
    }
    pub async fn count_package_versions(&self) -> usize {
        self.package_versions.lock().unwrap().len()
    }
}

#[async_trait]
impl ArtifactRepository for MockArtifactRepository {
    async fn find_physical_artifact_by_hash(
        &self,
        hash: &str,
    ) -> PortResult<Option<PhysicalArtifact>> {
        let artifacts = self.physical_artifacts.lock().unwrap();
        let found = artifacts
            .iter()
            .find(|a| a.content_hash.value == hash)
            .cloned();
        Ok(found)
    }

    async fn save_physical_artifact(&self, artifact: &PhysicalArtifact) -> PortResult<()> {
        if *self.should_fail_save_physical_artifact.lock().unwrap() {
            return Err(UploadArtifactError::RepositoryError(
                "Mock save_physical_artifact failed".to_string(),
            ));
        }
        self.physical_artifacts.lock().unwrap().push(artifact.clone());
        Ok(())
    }

    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()> {
        self.package_versions.lock().unwrap().push(package_version.clone());
        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct MockArtifactStorage {
    pub uploads: Arc<Mutex<Vec<(String, Bytes)>>>,
    pub should_fail_upload: Arc<Mutex<bool>>,
}

impl MockArtifactStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArtifactStorage for MockArtifactStorage {
    async fn upload(&self, content: Bytes, content_hash: &str) -> PortResult<String> {
        if *self.should_fail_upload.lock().unwrap() {
            return Err(UploadArtifactError::StorageError(
                "Mock upload failed".to_string(),
            ));
        }
        self.uploads
            .lock()
            .unwrap()
            .push((content_hash.to_string(), content));
        Ok(format!("mock://{}", content_hash))
    }

    async fn upload_from_path(&self, path: &Path, content_hash: &str) -> PortResult<String> {
        let content = tokio::fs::read(path).await.unwrap();
        self.upload(Bytes::from(content), content_hash).await
    }
}

#[derive(Default, Debug)]
pub struct MockEventPublisher {
    pub events: Arc<Mutex<Vec<ArtifactEvent>>>,
    pub should_fail_publish: Arc<Mutex<bool>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()> {
        if *self.should_fail_publish.lock().unwrap() {
            return Err(UploadArtifactError::EventError(
                "Mock publish failed".to_string(),
            ));
        }
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct MockArtifactValidator;

impl MockArtifactValidator {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArtifactValidator for MockArtifactValidator {
    async fn validate(
        &self,
        _command: &UploadArtifactCommand,
        _content: &Bytes,
    ) -> Result<(), Vec<String>> {
        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct MockVersionValidator;

impl MockVersionValidator {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl VersionValidator for MockVersionValidator {
    async fn validate_version(&self, version_str: &str) -> Result<(), String> {
        if version_str.contains("invalid") {
            Err("Invalid version".to_string())
        } else {
            Ok(())
        }
    }

    async fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, String> {
        Ok(ParsedVersion {
            original: version_str.to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: None,
            is_snapshot: false,
        })
    }
}