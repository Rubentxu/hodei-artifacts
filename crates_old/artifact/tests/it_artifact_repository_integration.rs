#![cfg(feature = "integration-mongo")]

//! Tests de integración para `MongoArtifactRepository`.
//!
//! Cobertura:
//! - save + get roundtrip.
//! - get inexistente → None.
//! - Índice único (repository_id + checksum) → ArtifactError::Duplicate.
//! - find_by_repo_and_checksum retorna artifact existente.
//! - ensure_indexes idempotente.
//! - Condición de carrera (dos inserts mismo repository_id+checksum) → 1 éxito + 1 Duplicate.
//!
//! Infra (TEST-TC5):
//! - Usa helper `infra_mongo::test_util::factory_from_env_or_container()` que intenta variables
//!   MONGO_URI + MONGO_DATABASE y si faltan levanta contenedor Mongo (testcontainers) 1 sola vez.
//!
//! Ejecutar:
//!   cargo test -p artifact --features integration-mongo --test it_artifact_repository_integration -- --nocapture

use std::sync::Arc;
use artifact::infrastructure::persistence::mongo_artifact_repository::MongoArtifactRepository;
use artifact::application::ports::ArtifactRepository;
use artifact::domain::model::{Artifact, ArtifactVersion, ArtifactChecksum};
use artifact::error::ArtifactError;
use shared::{RepositoryId, UserId};
use uuid::Uuid;

async fn build_repo() -> MongoArtifactRepository {
    let factory = infra_mongo::test_util::factory_from_env_or_container()
        .await
        .expect("factory (env o contenedor)");
    let repo = MongoArtifactRepository::new(Arc::new(factory));
    repo.ensure_indexes().await.expect("crear índices");
    repo
}

fn new_artifact(repo_id: RepositoryId, checksum: &str, version: &str) -> Artifact {
    Artifact::new(
        repo_id,
        ArtifactVersion(version.to_string()),
        format!("file-{version}.bin"),
        1234,
        ArtifactChecksum(checksum.to_string()),
        UserId(Uuid::new_v4()),
    )
}

#[tokio::test]
async fn save_and_get_roundtrip() {
    let repository = build_repo().await;
    let repo_id = RepositoryId(Uuid::new_v4());
    let art = new_artifact(repo_id, "sha256:AAA111", "1.0.0");
    repository.save(&art).await.expect("save ok");
    let fetched = repository.get(&art.id).await.expect("get ok").expect("debe existir");
    assert_eq!(fetched.id.0, art.id.0);
    assert_eq!(fetched.checksum.0, art.checksum.0);
    assert_eq!(fetched.version.0, "1.0.0");
}

#[tokio::test]
async fn get_nonexistent_returns_none() {
    let repository = build_repo().await;
    let missing = repository.get(&shared::ArtifactId(Uuid::new_v4())).await.expect("get ok");
    assert!(missing.is_none());
}

#[tokio::test]
async fn find_by_repo_and_checksum_returns_existing() {
    let repository = build_repo().await;
    let repo_id = RepositoryId(Uuid::new_v4());
    let checksum = "sha256:ABCDEF";
    let art = new_artifact(repo_id, checksum, "0.1.0");
    repository.save(&art).await.expect("save ok");
    let found = repository
        .find_by_repo_and_checksum(&repo_id, &ArtifactChecksum(checksum.to_string()))
        .await
        .expect("find ok")
        .expect("debe existir");
    assert_eq!(found.id.0, art.id.0);
    assert_eq!(found.checksum.0, checksum);
}

#[tokio::test]
async fn duplicate_repository_id_checksum_returns_error() {
    let repository = build_repo().await;
    let repo_id = RepositoryId(Uuid::new_v4());
    let checksum = "sha256:DUPLICATE";
    let a1 = new_artifact(repo_id, checksum, "1.0.0");
    let mut a2 = new_artifact(repo_id, checksum, "1.0.1"); // mismo repo+checksum ⇒ conflicto
    // Forzamos mismo checksum ya configurado, distinto version ok (índice no incluye version).
    repository.save(&a1).await.expect("primer insert");
    let err = repository.save(&a2).await.expect_err("debe fallar duplicado");
    match err {
        ArtifactError::Duplicate => {},
        other => panic!("Esperado Duplicate, obtenido: {other:?}")
    }
    // Verificar búsqueda idempotente
    let found = repository
        .find_by_repo_and_checksum(&repo_id, &ArtifactChecksum(checksum.to_string()))
        .await
        .expect("find ok")
        .expect("existente");
    assert_eq!(found.id.0, a1.id.0);
}

#[tokio::test]
async fn ensure_indexes_idempotent() {
    let repository = build_repo().await;
    repository.ensure_indexes().await.expect("segunda llamada idempotente");
}

#[tokio::test]
async fn concurrent_inserts_same_repo_checksum_only_one_succeeds() {
    let repository = build_repo().await;
    let repo_id = RepositoryId(Uuid::new_v4());
    let checksum = "sha256:RACE";
    let a1 = new_artifact(repo_id, checksum, "2.0.0");
    let a2 = new_artifact(repo_id, checksum, "2.0.1");
    let (r1, r2) = tokio::join!(repository.save(&a1), repository.save(&a2));
    let successes = r1.is_ok() as u8 + r2.is_ok() as u8;
    let failures = r1.is_err() as u8 + r2.is_err() as u8;
    assert_eq!(successes, 1, "exactamente un éxito");
    assert_eq!(failures, 1, "exactamente un fallo");
    for r in [r1, r2] {
        if let Err(e) = r {
            match e {
                ArtifactError::Duplicate => {},
                other => panic!("Error inesperado: {other:?}")
            }
        }
    }
    // Búsqueda idempotente posterior
    let found = repository
        .find_by_repo_and_checksum(&repo_id, &ArtifactChecksum(checksum.to_string()))
        .await
        .expect("find ok")
        .expect("debe existir");
    assert_eq!(found.checksum.0, checksum);
}