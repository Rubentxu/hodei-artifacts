use crate::features::create_scp::dto::CreateScpCommand;
use crate::features::create_scp::error::CreateScpError;
use crate::features::create_scp::mocks::MockCreateScpUnitOfWorkFactory;
use crate::features::create_scp::use_case::CreateScpUseCase;
use std::sync::Arc;

#[tokio::test]
async fn test_create_scp_success() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "TestSCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let scp_view = result.unwrap();
    assert_eq!(scp_view.name, "TestSCP");
    assert_eq!(scp_view.document, "permit(principal, action, resource);");
    assert!(scp_view.hrn.to_string().starts_with("hrn:"));
}

#[tokio::test]
async fn test_create_scp_empty_name() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateScpError::InvalidScpName));
}

#[tokio::test]
async fn test_create_scp_empty_document() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "TestSCP".to_string(),
        document: "".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateScpError::InvalidScpDocument));
}

#[tokio::test]
async fn test_create_scp_transaction_commit() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "TransactionalSCP".to_string(),
        document: "forbid(principal, action, resource);".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "TransactionalSCP");
    assert_eq!(view.document, "forbid(principal, action, resource);");
}

#[tokio::test]
async fn test_create_scp_transaction_rollback_on_failure() {
    // Arrange - Factory configured to fail on save
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::with_failure(true));
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "FailingSCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert - Should fail due to repository error
    assert!(result.is_err());
    match result.unwrap_err() {
        CreateScpError::ScpRepositoryError(_) => {
            // Expected error type
        }
        other => panic!("Expected ScpRepositoryError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_create_scp_generates_valid_hrn() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let command = CreateScpCommand {
        name: "HrnTestSCP".to_string(),
        document: "permit(principal, action, resource) when { true };".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();

    // Verify HRN structure
    let hrn_str = view.hrn.to_string();
    assert!(hrn_str.contains("aws"));
    assert!(hrn_str.contains("hodei"));
    assert!(hrn_str.contains("scp"));
    assert!(hrn_str.contains("HrnTestSCP"));
}

#[tokio::test]
async fn test_create_scp_with_complex_document() {
    // Arrange
    let uow_factory = Arc::new(MockCreateScpUnitOfWorkFactory::new());
    let use_case = CreateScpUseCase::new(uow_factory);

    let complex_document = r#"
        permit(
            principal,
            action == Action::"ReadData",
            resource
        ) when {
            principal.role == "admin"
        };
    "#;

    let command = CreateScpCommand {
        name: "ComplexSCP".to_string(),
        document: complex_document.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "ComplexSCP");
    assert_eq!(view.document, complex_document);
}
