use hodei_organizations::features::create_ou::use_case::CreateOuUseCase;
use hodei_organizations::features::create_ou::dto::CreateOuCommand;
use hodei_organizations::shared::infrastructure::surreal::ou_repository::SurrealOuRepository;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_ou_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear el repositorio
    let repository = SurrealOuRepository::new(db);
    
    // Arrange: Instanciar el caso de uso
    let use_case = CreateOuUseCase::new(Arc::new(repository));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "hodei".to_string(),
        "default".to_string(),
        "ou".to_string(),
        "parent-1".to_string(),
    );
    let command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que el OuView devuelto es correcto
    assert!(result.is_ok());
    let ou_view = result.unwrap();
    assert_eq!(ou_view.name, "TestOU");
    assert_eq!(ou_view.parent_hrn, parent_hrn);
    assert!(!ou_view.hrn.to_string().is_empty());
}
