#![cfg(feature = "integration-mongo")]

//! Tests de integración para MongoRepositoryStore:
//! - Inserción duplicada por `name` → RepositoryError::DuplicateName.
//! - Roundtrip save/get.
//! - get para id inexistente → None.
//! - ensure_indexes idempotente.
//! - Condición de carrera (dos inserciones simultáneas mismo nombre) produce exactamente 1 éxito + 1 error DuplicateName.
//!
//! Estrategia de infraestructura (TEST-TC3/TC4):
//! - Se intenta usar variables MONGO_URI + MONGO_DATABASE si existen (p.ej. CI con servicio Mongo).
//! - Si no existen, `infra_mongo::test_util::factory_from_env_or_container()` levanta (1 vez) un
//!   contenedor Mongo testcontainers y crea una base de datos aislada (nombre aleatorio) por test.
//! - Evita dependencia manual de entorno local y facilita reproducibilidad.
//!
//! Ejecución:
//!   cargo test -p repository --features integration-mongo --test it_repository_store_integration -- --nocapture
//!
//! Gating por feature integration-mongo para no ejecutar en suites unitarias por defecto.
use std::sync::Arc;
use repository::infrastructure::persistence::MongoRepositoryStore;
use repository::domain::model::{Repository, RepositoryName};
use repository::error::RepositoryError;
use shared::{RepositoryId, UserId};
use uuid::Uuid;
use repository::application::ports::RepositoryStore;
use infra_mongo::test_util::mongo_test_container::{ephemeral_store, TestMongoContainer};

/// Construye un store con factoría Mongo aislada (DB aleatoria) usando helper que
/// realiza fallback a testcontainers si no hay MONGO_* definidos.
/// Devuelve el store y el guard del contenedor para mantenerlo vivo durante el test.
async fn build_store() -> (MongoRepositoryStore, Option<TestMongoContainer>) {
    let (factory, container) = ephemeral_store()
        .await
        .expect("crear factory (env o contenedor)");
    let store = MongoRepositoryStore::new(Arc::new(factory));
    store.ensure_indexes().await.expect("crear índices");
    (store, container)
}

fn new_repo_with_name(name: &str) -> Repository {
    Repository::new(
        RepositoryId::new(),
        RepositoryName(name.to_string()),
        None,
        UserId::new(),
    )
}

#[tokio::test]
async fn save_and_get_roundtrip() {
    let (store, _container) = build_store().await;
    let repo = new_repo_with_name("roundtrip-repo");
    store.save(&repo).await.expect("save ok");
    let fetched = store.get(&repo.id).await.expect("get ok");
    let fetched = fetched.expect("debe existir");
    assert_eq!(fetched.name.0, repo.name.0);
    assert_eq!(fetched.id.0, repo.id.0);
}

#[tokio::test]
async fn get_nonexistent_returns_none() {
    let (store, _container) = build_store().await;
    let random_id = RepositoryId(Uuid::new_v4());
    let fetched = store.get(&random_id).await.expect("get ok");
    assert!(fetched.is_none(), "Debe ser None para id inexistente");
}

#[tokio::test]
async fn duplicate_name_returns_error() {
    let (store, _container) = build_store().await;
    let repo1 = new_repo_with_name("dup-repo");
    let repo2 = new_repo_with_name("dup-repo"); // mismo nombre, distinto id
    store.save(&repo1).await.expect("primer insert debe funcionar");
    let err = store.save(&repo2).await.expect_err("segundo insert debe fallar duplicado");
    match err {
        RepositoryError::DuplicateName => {},
        other => panic!("Se esperaba DuplicateName, obtenido: {other:?}")
    }
    let fetched = store.get(&repo1.id).await.unwrap().unwrap();
    assert_eq!(fetched.name.0, "dup-repo");
}

#[tokio::test]
async fn ensure_indexes_idempotent() {
    let (store, _container) = build_store().await;
    store.ensure_indexes().await.expect("idempotente");
}

#[tokio::test]
async fn concurrent_inserts_same_name_only_one_succeeds() {
    let (store, _container) = build_store().await;
    let name = "race-repo";
    let r1 = new_repo_with_name(name);
    let r2 = new_repo_with_name(name);

    let (res1, res2) = tokio::join!(store.save(&r1), store.save(&r2));

    let successes = res1.is_ok() as u8 + res2.is_ok() as u8;
    assert_eq!(successes, 1, "Debe haber exactamente un éxito");
    let failures = res1.is_err() as u8 + res2.is_err() as u8;
    assert_eq!(failures, 1, "Debe haber exactamente un fallo DuplicateName");

    for res in [res1, res2] {
        if let Err(e) = res {
            match e {
                RepositoryError::DuplicateName => {},
                other => panic!("Error inesperado en carrera: {other:?}")
            }
        }
    }
}
