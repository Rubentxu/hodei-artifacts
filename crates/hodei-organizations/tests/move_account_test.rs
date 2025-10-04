use hodei_organizations::features::move_account::use_case::MoveAccountUseCase;
use hodei_organizations::features::move_account::dto::MoveAccountCommand;
use hodei_organizations::shared::infrastructure::surreal::account_repository::SurrealAccountRepository;
use hodei_organizations::shared::infrastructure::surreal::ou_repository::SurrealOuRepository;
use hodei_organizations::shared::domain::ou::OrganizationalUnit;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use hodei_organizations::shared::domain::account::Account;
use policies::domain::Hrn;

#[tokio::test]
async fn test_move_account_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp", una OU "Staging" y una OU "Production"
    let staging_ou = OrganizationalUnit::new(
        "Staging".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-1".to_string()),
    );
    let production_ou = OrganizationalUnit::new(
        "Production".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-2".to_string()),
    );

    // Guardar las OUs
    ou_repository.save(&staging_ou).await.unwrap();
    ou_repository.save(&production_ou).await.unwrap();
    
    // Crear la cuenta WebApp inicialmente en Staging
    let webapp_account = Account::new("WebApp".to_string(), staging_ou.hrn.clone());
    
    // Guardar la cuenta
    account_repository.save(&webapp_account).await.unwrap();
    
    // Añadir la cuenta a la OU de Staging
    let mut staging_ou_with_account = staging_ou.clone();
    staging_ou_with_account.add_child_account(webapp_account.hrn.clone());
    ou_repository.save(&staging_ou_with_account).await.unwrap();
    
    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository.clone()), Arc::new(ou_repository.clone()));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: staging_ou_with_account.hrn.clone(),
        target_ou_hrn: production_ou.hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación fue exitosa
    assert!(result.is_ok());
    
    // Verificar que la cuenta se ha movido a Production
    let moved_account = account_repository.find_by_hrn(&webapp_account.hrn).await.unwrap().unwrap();
    assert_eq!(moved_account.parent_hrn, production_ou.hrn);
    
    // Verificar que la OU "Staging" ya no contiene la cuenta
    let updated_staging_ou = ou_repository.find_by_hrn(&staging_ou.hrn).await.unwrap().unwrap();
    assert!(!updated_staging_ou.child_accounts.contains(&webapp_account.hrn.to_string()));
    
    // Verificar que la OU "Production" ahora contiene la cuenta
    let updated_production_ou = ou_repository.find_by_hrn(&production_ou.hrn).await.unwrap().unwrap();
    assert!(updated_production_ou.child_accounts.contains(&webapp_account.hrn.to_string()));
}

#[tokio::test]
async fn test_move_account_source_not_found() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp" y una OU "Production"
    let production_ou = OrganizationalUnit::new(
        "Production".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-3".to_string()),
    );
    ou_repository.save(&production_ou).await.unwrap();
    
    let webapp_account = Account::new(
        "WebApp".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "ou-1".to_string()),
    );
    account_repository.save(&webapp_account).await.unwrap();
    
    // Create a non-existent source OU HRN
    let non_existent_ou_hrn = Hrn::new(
        "aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "non-existent".to_string(),
    );

    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository), Arc::new(ou_repository));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: non_existent_ou_hrn.clone(),
        target_ou_hrn: production_ou.hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación falló
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Source OU not found");
}

#[tokio::test]
async fn test_move_account_target_not_found() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp" y una OU "Staging"
    let staging_ou = OrganizationalUnit::new(
        "Staging".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-4".to_string()),
    );
    ou_repository.save(&staging_ou).await.unwrap();
    
    let webapp_account = Account::new("WebApp".to_string(), staging_ou.hrn.clone());
    account_repository.save(&webapp_account).await.unwrap();
    
    // Añadir la cuenta a la OU de Staging
    let mut staging_ou_with_account = staging_ou.clone();
    staging_ou_with_account.add_child_account(webapp_account.hrn.clone());
    ou_repository.save(&staging_ou_with_account).await.unwrap();
    
    // Create a non-existent target OU HRN
    let non_existent_ou_hrn = Hrn::new(
        "aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "non-existent".to_string(),
    );

    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository), Arc::new(ou_repository));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: staging_ou_with_account.hrn.clone(),
        target_ou_hrn: non_existent_ou_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación falló
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Target OU not found");
}