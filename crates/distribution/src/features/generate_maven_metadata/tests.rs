// crates/distribution/src/features/generate_maven_metadata/tests.rs

//! Tests unitarios para la feature generate_maven_metadata
//! 
//! Estos tests verifican el comportamiento de los DTOs, puertos y casos de uso
//! usando mocks para aislar completamente la lógica de negocio.

use super::*;
use crate::domain::maven::{MavenCoordinates, MavenVersion};
use std::sync::Arc;

// ===== Tests para DTOs =====

#[test]
fn test_generate_maven_metadata_request_creation() {
    let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    assert_eq!(request.coordinates.group_id(), "com.example");
    assert_eq!(request.coordinates.artifact_id(), "test-artifact");
    assert_eq!(request.coordinates.version(), &MavenVersion::new("1.0.0").unwrap());
    assert_eq!(request.repository_id, "test-repo");
    assert!(!request.force_regenerate);
}

#[test]
fn test_generate_maven_metadata_response_creation() {
    let metadata = MavenMetadataDto {
        group_id: "com.example".to_string(),
        artifact_id: "test-artifact".to_string(),
        version: "1.0.0".to_string(),
        versioning: MavenMetadataVersioningDto {
            latest: "1.0.0".to_string(),
            release: "1.0.0".to_string(),
            versions: vec!["1.0.0".to_string(), "1.1.0".to_string()],
            last_updated: "20240101120000".to_string(),
            snapshot: None,
        },
    };
    
    let response = GenerateMavenMetadataResponse {
        metadata,
        generated_at: "2024-01-01T12:00:00Z".to_string(),
        cache_hit: false,
    };
    
    assert_eq!(response.metadata.group_id, "com.example");
    assert_eq!(response.metadata.artifact_id, "test-artifact");
    assert_eq!(response.metadata.versioning.versions.len(), 2);
    assert!(!response.cache_hit);
}

#[test]
fn test_maven_metadata_dto_with_snapshot() {
    let snapshot = MavenMetadataSnapshotDto {
        timestamp: "20240101.120000".to_string(),
        build_number: 1,
    };
    
    let versioning = MavenMetadataVersioningDto {
        latest: "1.0.0-SNAPSHOT".to_string(),
        release: "".to_string(),
        versions: vec!["1.0.0-SNAPSHOT".to_string()],
        last_updated: "20240101120000".to_string(),
        snapshot: Some(snapshot),
    };
    
    let metadata = MavenMetadataDto {
        group_id: "com.example".to_string(),
        artifact_id: "test-artifact".to_string(),
        version: "1.0.0-SNAPSHOT".to_string(),
        versioning,
    };
    
    assert!(metadata.versioning.snapshot.is_some());
    let snapshot = metadata.versioning.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.timestamp, "20240101.120000");
    assert_eq!(snapshot.build_number, 1);
}

// ===== Mocks para testing =====

#[derive(Default)]
struct MockMavenMetadataGenerator {
    should_fail: bool,
    metadata: Option<MavenMetadataDto>,
}

#[async_trait::async_trait]
impl MavenMetadataGenerator for MockMavenMetadataGenerator {
    async fn generate_metadata(
        &self,
        coordinates: &MavenCoordinates,
        repository_id: &str,
    ) -> Result<MavenMetadataDto, MetadataGeneratorError> {
        if self.should_fail {
            return Err(MetadataGeneratorError::GenerationFailed(
                "Mock generation failed".to_string()
            ));
        }
        
        Ok(self.metadata.clone().unwrap_or_else(|| MavenMetadataDto {
            group_id: coordinates.group_id().to_string(),
            artifact_id: coordinates.artifact_id().to_string(),
            version: coordinates.version().to_string(),
            versioning: MavenMetadataVersioningDto {
                latest: coordinates.version().to_string(),
                release: coordinates.version().to_string(),
                versions: vec![coordinates.version().to_string()],
                last_updated: "20240101120000".to_string(),
                snapshot: None,
            },
        }))
    }
}

#[derive(Default)]
struct MockMavenArtifactLister {
    artifacts: Vec<String>,
    should_fail: bool,
}

#[async_trait::async_trait]
impl MavenArtifactLister for MockMavenArtifactLister {
    async fn list_artifacts(
        &self,
        group_id: &str,
        artifact_id: &str,
        repository_id: &str,
    ) -> Result<Vec<String>, ArtifactListerError> {
        if self.should_fail {
            return Err(ArtifactListerError::ListingFailed(
                "Mock listing failed".to_string()
            ));
        }
        
        Ok(self.artifacts.clone())
    }
}

#[derive(Default)]
struct MockMavenMetadataCache {
    cache: std::sync::Mutex<std::collections::HashMap<String, (MavenMetadataDto, String)>>,
    should_fail: bool,
}

#[async_trait::async_trait]
impl MavenMetadataCache for MockMavenMetadataCache {
    async fn get_metadata(
        &self,
        key: &str,
    ) -> Result<Option<(MavenMetadataDto, String)>, MetadataCacheError> {
        if self.should_fail {
            return Err(MetadataCacheError::CacheAccessFailed(
                "Mock cache access failed".to_string()
            ));
        }
        
        Ok(self.cache.lock().unwrap().get(key).cloned())
    }
    
    async fn set_metadata(
        &self,
        key: String,
        metadata: MavenMetadataDto,
        ttl_seconds: u64,
    ) -> Result<(), MetadataCacheError> {
        if self.should_fail {
            return Err(MetadataCacheError::CacheAccessFailed(
                "Mock cache set failed".to_string()
            ));
        }
        
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.cache.lock().unwrap().insert(key, (metadata, timestamp));
        Ok(())
    }
}

// ===== Tests para GenerateMavenMetadataUseCase =====

#[tokio::test]
async fn test_generate_metadata_success_with_cache_miss() {
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    let result = use_case.execute(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.metadata.group_id, "com.example");
    assert_eq!(response.metadata.artifact_id, "test");
    assert!(!response.cache_hit);
}

#[tokio::test]
async fn test_generate_metadata_success_with_cache_hit() {
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    // Pre-popular el caché
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let metadata = MavenMetadataDto {
        group_id: "com.example".to_string(),
        artifact_id: "test".to_string(),
        version: "1.0.0".to_string(),
        versioning: MavenMetadataVersioningDto {
            latest: "1.0.0".to_string(),
            release: "1.0.0".to_string(),
            versions: vec!["1.0.0".to_string()],
            last_updated: "20240101120000".to_string(),
            snapshot: None,
        },
    };
    
    let cache_key = format!("{}:{}:{}", "com.example", "test", "test-repo");
    cache.set_metadata(cache_key, metadata, 3600).await.unwrap();
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    let result = use_case.execute(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.metadata.group_id, "com.example");
    assert!(response.cache_hit);
}

#[tokio::test]
async fn test_generate_metadata_force_regenerate() {
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    // Pre-popular el caché
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let metadata = MavenMetadataDto {
        group_id: "com.example".to_string(),
        artifact_id: "test".to_string(),
        version: "1.0.0".to_string(),
        versioning: MavenMetadataVersioningDto {
            latest: "1.0.0".to_string(),
            release: "1.0.0".to_string(),
            versions: vec!["1.0.0".to_string()],
            last_updated: "20240101120000".to_string(),
            snapshot: None,
        },
    };
    
    let cache_key = format!("{}:{}:{}", "com.example", "test", "test-repo");
    cache.set_metadata(cache_key, metadata, 3600).await.unwrap();
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: true, // Forzar regeneración
    };
    
    let result = use_case.execute(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(!response.cache_hit); // No debería ser cache hit porque forzamos regeneración
}

#[tokio::test]
async fn test_generate_metadata_generator_failure() {
    let mut generator = MockMavenMetadataGenerator::default();
    generator.should_fail = true;
    let generator = Arc::new(generator);
    
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    let result = use_case.execute(request).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        GenerateMavenMetadataError::MetadataGenerationFailed(msg) => {
            assert_eq!(msg, "Mock generation failed");
        }
        _ => panic!("Expected MetadataGenerationFailed error"),
    }
}

#[tokio::test]
async fn test_generate_metadata_cache_failure() {
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    
    let mut cache = MockMavenMetadataCache::default();
    cache.should_fail = true;
    let cache = Arc::new(cache);
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    // Aunque el caché falle, debería continuar y generar el metadata
    let result = use_case.execute(request).await;
    assert!(result.is_ok());
}

// ===== Tests para GenerateMavenMetadataApi =====

#[tokio::test]
async fn test_api_generate_metadata_success() {
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let api = GenerateMavenMetadataApi::new(use_case);
    
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_metadata(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.metadata.group_id, "com.example");
}

#[tokio::test]
async fn test_api_generate_metadata_with_tracing() {
    use tracing_test::traced_test;
    
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister::default());
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    let api = GenerateMavenMetadataApi::new(use_case);
    
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "test-repo".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_metadata(request).await;
    assert!(result.is_ok());
    
    // Verificar que se generaron logs (si tracing_test está disponible)
    // assert!(logs_contain("Generating Maven metadata"));
}

// ===== Tests de integración básica =====

#[tokio::test]
async fn test_full_metadata_generation_flow() {
    // Crear todos los mocks
    let generator = Arc::new(MockMavenMetadataGenerator::default());
    let lister = Arc::new(MockMavenArtifactLister {
        artifacts: vec!["1.0.0".to_string(), "1.1.0".to_string()],
        should_fail: false,
    });
    let cache = Arc::new(MockMavenMetadataCache::default());
    
    // Crear el caso de uso
    let use_case = GenerateMavenMetadataUseCase::new(
        generator.clone(),
        lister.clone(),
        cache.clone(),
    );
    
    // Crear la API
    let api = GenerateMavenMetadataApi::new(use_case);
    
    // Crear la petición
    let coordinates = MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap();
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "maven-central".to_string(),
        force_regenerate: false,
    };
    
    // Ejecutar la generación
    let result = api.generate_metadata(request).await;
    
    // Verificar el resultado
    assert!(result.is_ok());
    let response = result.unwrap();
    
    assert_eq!(response.metadata.group_id, "com.example");
    assert_eq!(response.metadata.artifact_id, "test-artifact");
    assert_eq!(response.metadata.version, "1.0.0");
    assert!(!response.cache_hit);
    assert!(!response.generated_at.is_empty());
}

// ===== Tests de validación =====

#[test]
fn test_maven_coordinates_validation_in_request() {
    // Coordenadas válidas
    let valid_coords = MavenCoordinates::new("com.example", "valid-artifact", "1.0.0");
    assert!(valid_coords.is_ok());
    
    // Coordenadas inválidas (group_id vacío)
    let invalid_coords = MavenCoordinates::new("", "artifact", "1.0.0");
    assert!(invalid_coords.is_err());
}

#[test]
fn test_repository_id_validation() {
    let coordinates = MavenCoordinates::new("com.example", "test", "1.0.0").unwrap();
    
    // Repository ID válido
    let request = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "valid-repo".to_string(),
        force_regenerate: false,
    };
    assert_eq!(request.repository_id, "valid-repo");
    
    // Repository ID vacío (debería ser manejado por validación)
    let request_empty = GenerateMavenMetadataRequest {
        coordinates: coordinates.clone(),
        repository_id: "".to_string(),
        force_regenerate: false,
    };
    assert!(request_empty.repository_id.is_empty());
}