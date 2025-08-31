//! Test de condiciones de carrera para idempotencia de upload de artifacts
//! IDEMP-2: Verificar que inserciones concurrentes del mismo artifact retornan el mismo ID

#![cfg(feature = "integration-mongo")]

use std::sync::Arc;
use tokio::time::{timeout, Duration};
use uuid::Uuid;
use artifact::{
    application::ports::ArtifactRepository,
    domain::model::{Artifact, ArtifactChecksum, ArtifactVersion},
    error::ArtifactError,
    infrastructure::persistence::MongoArtifactRepository,
};
use shared_test::{setup_test_environment, TestEnvironment};
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};

/// Construye un repositorio de artifacts usando el entorno de test compartido
async fn build_artifact_repository_for_test() -> (Arc<MongoArtifactRepository>, TestEnvironment) {
    let test_env = setup_test_environment(None).await;
    (test_env.artifact_repository, test_env)
}

#[tokio::test]
async fn test_concurrent_upload_same_artifact_returns_same_id() {
    let (repo, _test_env) = build_artifact_repository_for_test().await;
    
    // Crear un artifact de prueba
    let repository_id = RepositoryId(Uuid::new_v4());
    let checksum = ArtifactChecksum("a".repeat(64));
    let artifact_id = ArtifactId(Uuid::new_v4());
    
    let artifact = Artifact {
        id: artifact_id,
        repository_id: repository_id.clone(),
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test.jar".to_string(),
        size_bytes: 1024,
        checksum: checksum.clone(),
        created_at: IsoTimestamp(chrono::Utc::now()),
        created_by: UserId(Uuid::new_v4()),
        coordinates: None,
    };

    // Ejecutar 10 inserciones concurrentes del mismo artifact
    let mut handles = vec![];
    for _ in 0..10 {
        let repo_clone = Arc::clone(&repo);
        let artifact_clone = artifact.clone();
        
        let handle = tokio::spawn(async move {
            repo_clone.save(&artifact_clone).await
        });
        handles.push(handle);
    }

    // Recoger resultados
    let mut results = vec![];
    for handle in handles {
        let result = timeout(Duration::from_secs(5), handle)
            .await
            .expect("Timeout en insert concurrente")
            .expect("Task panic");
        results.push(result);
    }

    // Verificar resultados: debe haber exactamente un éxito y el resto duplicados
    let successes: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
    let duplicates: Vec<_> = results.iter().filter(|r| {
        matches!(r, Err(ArtifactError::Duplicate))
    }).collect();

    assert_eq!(successes.len(), 1, "Debe haber exactamente un insert exitoso");
    assert_eq!(duplicates.len(), 9, "Debe haber 9 errores de duplicado");

    // Verificar que el artifact se puede recuperar
    let found = repo.get(&artifact.id).await.expect("Error al buscar artifact");
    assert!(found.is_some(), "El artifact debe existir en la BD");
    
    let found_artifact = found.unwrap();
    assert_eq!(found_artifact.id, artifact.id);
    assert_eq!(found_artifact.repository_id, repository_id);
    assert_eq!(found_artifact.checksum.0, checksum.0);
}

#[tokio::test]
async fn test_find_by_repo_and_checksum_idempotency() {
    let (repo, _container) = build_artifact_repository_for_test().await;
    
    let repository_id = RepositoryId(Uuid::new_v4());
    let checksum = ArtifactChecksum("b".repeat(64));
    
    // Verificar que no existe inicialmente
    let not_found = repo.find_by_repo_and_checksum(&repository_id, &checksum)
        .await
        .expect("Error en find_by_repo_and_checksum");
    assert!(not_found.is_none());
    
    // Crear y guardar artifact
    let artifact = Artifact {
        id: ArtifactId(Uuid::new_v4()),
        repository_id: repository_id.clone(),
        version: ArtifactVersion("2.0.0".to_string()),
        file_name: "test2.jar".to_string(),
        size_bytes: 2048,
        checksum: checksum.clone(),
        created_at: IsoTimestamp(chrono::Utc::now()),
        created_by: UserId(Uuid::new_v4()),
        coordinates: None,
    };
    
    repo.save(&artifact).await.expect("Error al guardar artifact");
    
    // Verificar que ahora se encuentra
    let found = repo.find_by_repo_and_checksum(&repository_id, &checksum)
        .await
        .expect("Error en find_by_repo_and_checksum");
    assert!(found.is_some());
    
    let found_artifact = found.unwrap();
    assert_eq!(found_artifact.id, artifact.id);
    assert_eq!(found_artifact.checksum.0, checksum.0);
    
    // Intentar guardar de nuevo debe fallar con Duplicate
    let duplicate_result = repo.save(&artifact).await;
    assert!(matches!(duplicate_result, Err(ArtifactError::Duplicate)));
}

#[tokio::test]
async fn test_different_repos_same_checksum_allowed() {
    let (repo, _container) = build_artifact_repository_for_test().await;
    
    let checksum = ArtifactChecksum("c".repeat(64));
    let repo_id1 = RepositoryId(Uuid::new_v4());
    let repo_id2 = RepositoryId(Uuid::new_v4());
    
    let artifact1 = Artifact {
        id: ArtifactId(Uuid::new_v4()),
        repository_id: repo_id1,
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test.jar".to_string(),
        size_bytes: 1024,
        checksum: checksum.clone(),
        created_at: IsoTimestamp(chrono::Utc::now()),
        created_by: UserId(Uuid::new_v4()),
        coordinates: None,
    };
    
    let artifact2 = Artifact {
        id: ArtifactId(Uuid::new_v4()),
        repository_id: repo_id2,
        version: ArtifactVersion("1.0.0".to_string()),
        file_name: "test.jar".to_string(),
        size_bytes: 1024,
        checksum: checksum.clone(),
        created_at: IsoTimestamp(chrono::Utc::now()),
        created_by: UserId(Uuid::new_v4()),
        coordinates: None,
    };
    
    // Ambos deben poder guardarse (diferentes repositorios)
    repo.save(&artifact1).await.expect("Error al guardar artifact1");
    repo.save(&artifact2).await.expect("Error al guardar artifact2");
    
    // Verificar que ambos existen
    let found1 = repo.get(&artifact1.id).await.expect("Error al buscar artifact1");
    let found2 = repo.get(&artifact2.id).await.expect("Error al buscar artifact2");
    
    assert!(found1.is_some());
    assert!(found2.is_some());
}
