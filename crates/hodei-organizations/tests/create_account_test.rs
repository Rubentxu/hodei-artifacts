use hodei_organizations::features::create_account::use_case::CreateAccountUseCase;
use hodei_organizations::features::create_account::dto::CreateAccountCommand;
use hodei_organizations::shared::infrastructure::surreal::account_repository::SurrealAccountRepository;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_account_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear el repositorio
    let repository = SurrealAccountRepository::new(db);
    
    // Arrange: Instanciar el caso de uso
    let use_case = CreateAccountUseCase::new(Arc::new(repository));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "hodei".to_string(),
        "default".to_string(),
        "ou".to_string(),
        "parent-1".to_string(),
    );
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: parent_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que el AccountView devuelto es correcto
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, parent_hrn);
    assert!(!account_view.hrn.to_string().is_empty());
}
