use crate::features::create_ou::dto::CreateOuCommand;
use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::mocks::MockCreateOuUnitOfWorkFactory;
use crate::features::create_ou::use_case::CreateOuUseCase;
use policies::domain::Hrn;
use std::sync::Arc;

#[tokio::test]
async fn test_create_ou_success() {
    // Arrange
    let uow_factory = Arc::new(MockCreateOuUnitOfWorkFactory::new());
    let use_case = CreateOuUseCase::new(uow_factory);

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-123".to_string(),
    );

    let command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let ou_view = result.unwrap();
    assert_eq!(ou_view.name, "TestOU");
    assert_eq!(ou_view.parent_hrn, parent_hrn);
    assert!(!ou_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_ou_empty_name() {
    // Arrange
    let uow_factory = Arc::new(MockCreateOuUnitOfWorkFactory::new());
    let use_case = CreateOuUseCase::new(uow_factory);

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-123".to_string(),
    );

    let command = CreateOuCommand {
        name: "".to_string(),
        parent_hrn,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateOuError::InvalidOuName));
}

#[tokio::test]
async fn test_create_ou_transaction_commit() {
    // Arrange
    let uow_factory = Arc::new(MockCreateOuUnitOfWorkFactory::new());
    let use_case = CreateOuUseCase::new(uow_factory);

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-456".to_string(),
    );

    let command = CreateOuCommand {
        name: "TransactionalOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "TransactionalOU");
    assert_eq!(view.parent_hrn, parent_hrn);
}

#[tokio::test]
async fn test_create_ou_transaction_rollback_on_failure() {
    // Arrange - Factory configured to fail on save
    let uow_factory = Arc::new(MockCreateOuUnitOfWorkFactory::with_failure(true));
    let use_case = CreateOuUseCase::new(uow_factory);

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-789".to_string(),
    );

    let command = CreateOuCommand {
        name: "FailingOU".to_string(),
        parent_hrn,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert - Should fail due to repository error
    assert!(result.is_err());
    match result.unwrap_err() {
        CreateOuError::OuRepositoryError(_) => {
            // Expected error type
        }
        other => panic!("Expected OuRepositoryError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_create_ou_generates_valid_hrn() {
    // Arrange
    let uow_factory = Arc::new(MockCreateOuUnitOfWorkFactory::new());
    let use_case = CreateOuUseCase::new(uow_factory);

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "987654321098".to_string(),
        "root".to_string(),
        "r-root".to_string(),
    );

    let command = CreateOuCommand {
        name: "HrnTestOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();

    // Verify HRN structure
    let hrn_str = view.hrn.to_string();
    assert!(hrn_str.contains("organizations"));
    assert!(hrn_str.contains("ou"));
    assert!(hrn_str.contains("HrnTestOU"));
    assert_eq!(view.parent_hrn, parent_hrn);
}
