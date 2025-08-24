use artifact::{
    application::ports::{ArtifactRepository, ArtifactStorage, ArtifactEventPublisher},
    domain::model::{Artifact, ArtifactChecksum, ArtifactVersion},
    error::ArtifactError,
    features::download_artifact::{
        query::{GetArtifactQuery, DownloadMethod},
        logic::handle_get_artifact,
    },
};
use async_trait::async_trait;
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio;
use uuid::Uuid;

// Mock implementations para testing
#[derive(Debug, Clone)]
struct MockArtifactRepository {
    artifacts: Arc<Mutex<HashMap<ArtifactId, Artifact>>>,
}

impl MockArtifactRepository {
    fn new() -> Self {
        Self {
            artifacts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_artifact(&self, artifact: Artifact) {
        let mut artifacts = self.artifacts.lock().unwrap();
        artifacts.insert(artifact.id, artifact);
    }
}

#[async_trait]
impl ArtifactRepository for MockArtifactRepository {
    async fn save(&self, _artifact: &Artifact) -> Result<(), ArtifactError> {
        Ok(())
    }

    async fn get(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError> {
        let artifacts = self.artifacts.lock().unwrap();
        Ok(artifacts.get(id).cloned())
    }

    async fn find_by_repo_and_checksum(&self, _repository: &RepositoryId, _checksum: &ArtifactChecksum) -> Result<Option<Artifact>, ArtifactError> {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
struct MockArtifactStorage {
    objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockArtifactStorage {
    fn new() -> Self {
        Self {
            objects: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_object(&self, key: &str, content: Vec<u8>) {
        let mut objects = self.objects.lock().unwrap();
        objects.insert(key.to_string(), content);
    }
}

#[async_trait]
impl ArtifactStorage for MockArtifactStorage {
    async fn put_object(
        &self,
        _repository: &RepositoryId,
        _artifact_id: &ArtifactId,
        _bytes: &[u8],
    ) -> Result<(), ArtifactError> {
        Ok(())
    }

    async fn get_object_stream(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
    ) -> Result<Vec<u8>, ArtifactError> {
        let key = format!("{}/{}", repository_id.0, artifact_id.0);
        let objects = self.objects.lock().unwrap();
        objects
            .get(&key)
            .cloned()
            .ok_or(ArtifactError::NotFound)
    }

    async fn get_presigned_download_url(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        expires_in_secs: u64,
    ) -> Result<String, ArtifactError> {
        let key = format!("{}/{}", repository_id.0, artifact_id.0);
        let objects = self.objects.lock().unwrap();
        
        if objects.contains_key(&key) {
            let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in_secs as i64);
            Ok(format!(
                "https://s3.amazonaws.com/test-bucket/{}?expires_at={}",
                key,
                expires_at.timestamp()
            ))
        } else {
            Err(ArtifactError::NotFound)
        }
    }
}

#[derive(Debug, Clone)]
struct MockEventPublisher {
    published_events: Arc<Mutex<Vec<String>>>,
}

impl MockEventPublisher {
    fn new() -> Self {
        Self {
            published_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_published_events(&self) -> Vec<String> {
        self.published_events.lock().unwrap().clone()
    }
}

#[async_trait]
impl ArtifactEventPublisher for MockEventPublisher {
    async fn publish_created(&self, _artifact: &Artifact) -> Result<(), ArtifactError> {
        Ok(())
    }

    async fn publish_download_requested(
        &self,
        event: &shared::domain::event::ArtifactDownloadRequestedEvent,
    ) -> Result<(), ArtifactError> {
        let mut events = self.published_events.lock().unwrap();
        events.push(format!("download_requested:{}", event.data.artifact_id.0));
        Ok(())
    }
}

fn create_test_artifact() -> Artifact {
    let artifact_id = ArtifactId(Uuid::new_v4());
    let repository_id = RepositoryId(Uuid::new_v4());
    let user_id = UserId(Uuid::new_v4());

    Artifact {
        id: artifact_id,
        repository_id,
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test-artifact.jar".to_string(),
        size_bytes: 1024,
        checksum: ArtifactChecksum("abc123def456".to_string()),
        created_at: IsoTimestamp::now(),
        created_by: user_id,
        coordinates: None,
    }
}

#[tokio::test]
async fn test_download_artifact_direct_success() {
    // Arrange
    let artifact = create_test_artifact();
    let test_content = b"test artifact content".to_vec();

    let repository = MockArtifactRepository::new();
    repository.add_artifact(artifact.clone());

    let storage = MockArtifactStorage::new();
    let storage_key = format!("{}/{}", artifact.repository_id.0, artifact.id.0);
    storage.add_object(&storage_key, test_content.clone());

    let event_publisher = MockEventPublisher::new();

    let query = GetArtifactQuery::new(artifact.id, artifact.created_by)
        .with_user_agent("test-agent".to_string())
        .with_client_ip("127.0.0.1".to_string());

    // Act
    let result = handle_get_artifact(
        query,
        &repository,
        &storage,
        &event_publisher,
    ).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    
    assert_eq!(response.artifact_id, artifact.id);
    assert_eq!(response.file_name, artifact.file_name);
    assert_eq!(response.size_bytes, artifact.size_bytes);
    assert_eq!(response.checksum, artifact.checksum.0);

    match response.download_method {
        DownloadMethod::Direct { content } => {
            assert_eq!(content, test_content);
        }
        _ => panic!("Expected direct download method"),
    }

    // Verificar que se publicó el evento
    let events = event_publisher.get_published_events();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("download_requested"));
}

#[tokio::test]
async fn test_download_artifact_presigned_success() {
    // Arrange
    let artifact = create_test_artifact();
    let test_content = b"test artifact content".to_vec();

    let repository = MockArtifactRepository::new();
    repository.add_artifact(artifact.clone());

    let storage = MockArtifactStorage::new();
    let storage_key = format!("{}/{}", artifact.repository_id.0, artifact.id.0);
    storage.add_object(&storage_key, test_content.clone());

    let event_publisher = MockEventPublisher::new();

    let query = GetArtifactQuery::new(artifact.id, artifact.created_by)
        .with_presigned(3600)
        .with_user_agent("test-agent".to_string())
        .with_client_ip("127.0.0.1".to_string());

    // Act
    let result = handle_get_artifact(
        query,
        &repository,
        &storage,
        &event_publisher,
    ).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    
    assert_eq!(response.artifact_id, artifact.id);
    assert_eq!(response.file_name, artifact.file_name);
    assert_eq!(response.size_bytes, artifact.size_bytes);
    assert_eq!(response.checksum, artifact.checksum.0);

    match response.download_method {
        DownloadMethod::PresignedUrl { url, expires_at } => {
            assert!(url.contains("s3.amazonaws.com"));
            assert!(url.contains(&storage_key));
            assert!(!expires_at.is_empty());
        }
        _ => panic!("Expected presigned URL download method"),
    }

    // Verificar que se publicó el evento
    let events = event_publisher.get_published_events();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("download_requested"));
}

#[tokio::test]
async fn test_download_artifact_not_found() {
    // Arrange
    let artifact_id = ArtifactId(Uuid::new_v4());
    let user_id = UserId(Uuid::new_v4());

    let repository = MockArtifactRepository::new();
    let storage = MockArtifactStorage::new();
    let event_publisher = MockEventPublisher::new();

    let query = GetArtifactQuery::new(artifact_id, user_id)
        .with_user_agent("test-agent".to_string())
        .with_client_ip("127.0.0.1".to_string());

    // Act
    let result = handle_get_artifact(
        query,
        &repository,
        &storage,
        &event_publisher,
    ).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        ArtifactError::NotFound => {
            // Expected error
        }
        other => panic!("Expected NotFound error, got: {:?}", other),
    }

    // Verificar que no se publicó ningún evento
    let events = event_publisher.get_published_events();
    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_download_artifact_storage_error() {
    // Arrange
    let artifact = create_test_artifact();

    let repository = MockArtifactRepository::new();
    repository.add_artifact(artifact.clone());

    let storage = MockArtifactStorage::new();
    // No agregamos el objeto al storage para simular error

    let event_publisher = MockEventPublisher::new();

    let query = GetArtifactQuery::new(artifact.id, artifact.created_by)
        .with_user_agent("test-agent".to_string())
        .with_client_ip("127.0.0.1".to_string());

    // Act
    let result = handle_get_artifact(
        query,
        &repository,
        &storage,
        &event_publisher,
    ).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        ArtifactError::NotFound => {
            // Expected error cuando el archivo no existe en storage
        }
        other => panic!("Expected NotFound error, got: {:?}", other),
    }

    // Verificar que se publicó el evento de intento de descarga
    let events = event_publisher.get_published_events();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("download_requested"));
}
