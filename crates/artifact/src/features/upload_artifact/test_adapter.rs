use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::domain::{
    package_version::PackageVersion,
    physical_artifact::PhysicalArtifact,
    events::ArtifactEvent,
};
use super::ports::{ArtifactRepository, ArtifactStorage, EventPublisher, PortResult};
use std::path::Path;
use super::error::UploadArtifactError;

pub struct MockArtifactRepository {
    physical_artifacts: Mutex<HashMap<String, PhysicalArtifact>>,
    package_versions: Mutex<HashMap<String, PackageVersion>>,
    pub should_fail_save_package_version: Mutex<bool>,
    pub should_fail_save_physical_artifact: Mutex<bool>,
}

impl MockArtifactRepository {
    pub fn new() -> Self {
        Self {
            physical_artifacts: Mutex::new(HashMap::new()),
            package_versions: Mutex::new(HashMap::new()),
            should_fail_save_package_version: Mutex::new(false),
            should_fail_save_physical_artifact: Mutex::new(false),
        }
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
    async fn save_package_version(&self, package_version: &PackageVersion) -> PortResult<()> {
        if *self.should_fail_save_package_version.lock().unwrap() {
            tracing::error!("Mock save_package_version failed");
            return Err(UploadArtifactError::RepositoryError("Mock save_package_version failed".to_string()));
        }
        tracing::debug!("Saving package version: {}", package_version.hrn);
        self.package_versions.lock().unwrap().insert(package_version.hrn.to_string(), package_version.clone());
        Ok(())
    }

    async fn save_physical_artifact(&self, physical_artifact: &PhysicalArtifact) -> PortResult<()> {
        tracing::debug!("MockArtifactRepository: save_physical_artifact called. should_fail_save_physical_artifact: {}", *self.should_fail_save_physical_artifact.lock().unwrap());
        if *self.should_fail_save_physical_artifact.lock().unwrap() {
            tracing::error!("Mock save_physical_artifact failed");
            return Err(UploadArtifactError::RepositoryError("Mock save_physical_artifact failed".to_string()));
        }
        tracing::debug!("Saving physical artifact with hash: {}", physical_artifact.content_hash.value);
        self.physical_artifacts.lock().unwrap().insert(physical_artifact.content_hash.value.clone(), physical_artifact.clone());
        Ok(())
    }

    async fn find_physical_artifact_by_hash(&self, hash: &str) -> PortResult<Option<PhysicalArtifact>> {
        tracing::debug!("Finding physical artifact by hash: {}", hash);
        let artifact = self.physical_artifacts.lock().unwrap().get(hash).cloned();
        if artifact.is_some() {
            tracing::debug!("Found existing physical artifact");
        } else {
            tracing::debug!("No existing physical artifact found");
        }
        Ok(artifact)
    }
}

pub struct MockArtifactStorage {
    pub should_fail_upload: Mutex<bool>,
}

impl MockArtifactStorage {
    pub fn new() -> Self {
        Self { should_fail_upload: Mutex::new(false) }
    }
}

#[async_trait]
impl ArtifactStorage for MockArtifactStorage {
    async fn upload(&self, _content: Bytes, content_hash: &str) -> PortResult<String> {
        if *self.should_fail_upload.lock().unwrap() {
            tracing::error!("Mock upload failed");
            return Err(UploadArtifactError::StorageError("Mock upload failed".to_string()));
        }
        tracing::debug!("Uploading content with hash: {}", content_hash);
        Ok(format!("s3://mock-bucket/{}", content_hash))
    }

    async fn upload_from_path(&self, _path: &Path, content_hash: &str) -> PortResult<String> {
        if *self.should_fail_upload.lock().unwrap() {
            tracing::error!("Mock upload_from_path failed");
            return Err(UploadArtifactError::StorageError("Mock upload_from_path failed".to_string()));
        }
        tracing::debug!("Uploading content from path with hash: {}", content_hash);
        Ok(format!("s3://mock-bucket/{}", content_hash))
    }
}

pub struct MockEventPublisher {
    pub events: Arc<Mutex<Vec<ArtifactEvent>>>,
    pub should_fail_publish: Mutex<bool>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self { 
            events: Arc::new(Mutex::new(Vec::new())),
            should_fail_publish: Mutex::new(false),
        }
    }
}

#[async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish(&self, event: &ArtifactEvent) -> PortResult<()> {
        if *self.should_fail_publish.lock().unwrap() {
            tracing::error!("Mock publish failed");
            return Err(UploadArtifactError::EventError("Mock publish failed".to_string()));
        }
        tracing::debug!("Publishing event: {:?}", event);
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}
