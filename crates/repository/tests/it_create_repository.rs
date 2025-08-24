//! Test de integración para el flujo de creación de repositorios.
//!
//! Este test verifica el happy path del `CreateRepositoryHandler`, asegurando
//! que la lógica de negocio, la persistencia en MongoDB y la publicación
//! de eventos (simulada) funcionan conjuntamente como se espera.
//!
//! Pasos del test:
//! 1. Levanta una instancia efímera de MongoDB usando `testcontainers`.
//! 2. Inicializa las implementaciones de infraestructura (`MongoRepositoryStore`, `LoggingEventBus`).
//! 3. Construye y ejecuta un `CreateRepositoryCommand`.
//! 4. Verifica que la respuesta del handler es correcta (`Ok(CreateRepositoryResponse)`).
//! 5. Comprueba directamente en la base de datos que el repositorio fue persistido.
//! 6. (Opcional) Verifica que un intento de crear un repositorio con el mismo nombre falla.

use repository::application::ports::RepositoryStore;
use repository::features::create_repository::{CreateRepositoryCommand, CreateRepositoryHandler};
use repository::infrastructure::messaging::LoggingEventBus;
use repository::infrastructure::persistence::MongoRepositoryStore;
use repository::error::RepositoryError;
use shared::UserId;
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use std::sync::Arc;

#[tokio::test]
async fn test_create_repository_happy_path() {
    // --- Arrange ---
    // 1. Setup: Base de datos efímera y dependencias.
    let (mongo_client_factory, _container) = ephemeral_store()
        .await
        .expect("No se pudo inicializar la BBDD efímera");

    let repo_store = Arc::new(MongoRepositoryStore::new(Arc::new(mongo_client_factory)));
    repo_store.ensure_indexes().await.unwrap();
    let event_bus = Arc::new(LoggingEventBus::new());

    let handler = CreateRepositoryHandler::new(repo_store.clone(), event_bus);

    // 2. Comando de entrada.
    let user_id = UserId::new();
    let cmd = CreateRepositoryCommand {
        name: "test-repo-integration".to_string(),
        description: Some("Un repositorio para el test de integración".to_string()),
        created_by: user_id,
    };

    // --- Act ---
    // 3. Ejecutar el handler.
    let response = handler.handle(cmd.clone()).await.unwrap();

    // --- Assert ---
    // 4. Verificar la respuesta.
    assert_eq!(response.name, cmd.name);
    assert_eq!(response.description, cmd.description);
    assert_eq!(response.created_by, cmd.created_by);

    // 5. Verificar directamente en la BBDD.
    let saved_repo = repo_store.get(&response.id).await.unwrap().unwrap();
    assert_eq!(saved_repo.name.0, cmd.name);
    assert_eq!(saved_repo.description.unwrap().0, cmd.description.unwrap());
    assert_eq!(saved_repo.created_by, cmd.created_by);
}

#[tokio::test]
async fn test_create_repository_duplicate_fails() {
    // --- Arrange ---
    let (mongo_client_factory, _container) = ephemeral_store().await.unwrap();
    let repo_store = Arc::new(MongoRepositoryStore::new(Arc::new(mongo_client_factory)));
    repo_store.ensure_indexes().await.unwrap();
    let event_bus = Arc::new(LoggingEventBus::new());
    let handler = CreateRepositoryHandler::new(repo_store.clone(), event_bus);

    let user_id = UserId::new();
    let cmd = CreateRepositoryCommand {
        name: "test-repo-duplicado".to_string(),
        description: None,
        created_by: user_id,
    };

    // Creamos el primero.
    handler.handle(cmd.clone()).await.unwrap();

    // --- Act & Assert ---
    // El segundo intento con el mismo nombre debe fallar.
    let result = handler.handle(cmd.clone()).await;
    assert!(matches!(
        result,
        Err(RepositoryError::DuplicateName)
    ));
}
