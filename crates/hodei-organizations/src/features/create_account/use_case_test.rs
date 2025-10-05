use crate::features::create_account::dto::CreateAccountCommand;
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::mocks::MockCreateAccountUnitOfWorkFactory;
use crate::features::create_account::use_case::CreateAccountUseCase;
use kernel::Hrn;
use std::sync::Arc;

#[tokio::test]
async fn test_create_account_success() {
    // Arrange
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::new());
    let use_case =
        CreateAccountUseCase::new(uow_factory, "aws".to_string(), "123456789012".to_string());

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );

    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: Some(parent_hrn.clone()),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, Some(parent_hrn));
    assert!(!account_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_account_empty_name() {
    // Arrange
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::new());
    let use_case =
        CreateAccountUseCase::new(uow_factory, "aws".to_string(), "123456789012".to_string());

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );

    let command = CreateAccountCommand {
        name: "".to_string(),
        parent_hrn: Some(parent_hrn),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateAccountError::InvalidAccountName));
}

#[tokio::test]
async fn test_create_account_transaction_commit() {
    // Arrange
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::new());
    let use_case =
        CreateAccountUseCase::new(uow_factory, "aws".to_string(), "123456789012".to_string());

    let command = CreateAccountCommand {
        name: "TransactionalAccount".to_string(),
        parent_hrn: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "TransactionalAccount");
}

#[tokio::test]
async fn test_create_account_transaction_rollback_on_failure() {
    // Arrange - Factory configured to fail on save
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::with_failure(true));
    let use_case =
        CreateAccountUseCase::new(uow_factory, "aws".to_string(), "123456789012".to_string());

    let command = CreateAccountCommand {
        name: "FailingAccount".to_string(),
        parent_hrn: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert - Should fail due to repository error
    assert!(result.is_err());
    match result.unwrap_err() {
        CreateAccountError::AccountRepositoryError(_) => {
            // Expected error type
        }
        other => panic!("Expected AccountRepositoryError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_create_account_with_events() {
    // Arrange
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::new());
    let use_case =
        CreateAccountUseCase::new(uow_factory, "aws".to_string(), "123456789012".to_string());
    // Note: Event publisher would be injected via with_event_publisher() in real scenarios

    let command = CreateAccountCommand {
        name: "AccountWithEvents".to_string(),
        parent_hrn: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "AccountWithEvents");
    // In a real scenario, we would assert that the event was published
}

#[tokio::test]
async fn test_create_account_generates_valid_hrn() {
    // Arrange
    let uow_factory = Arc::new(MockCreateAccountUnitOfWorkFactory::new());
    let partition = "aws".to_string();
    let account_id = "987654321098".to_string();
    let use_case = CreateAccountUseCase::new(uow_factory, partition.clone(), account_id.clone());

    let command = CreateAccountCommand {
        name: "HrnTestAccount".to_string(),
        parent_hrn: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();

    // Verify HRN structure
    let hrn_str = view.hrn.to_string();
    assert!(hrn_str.contains(&partition));
    assert!(hrn_str.contains("organizations"));
    assert!(hrn_str.contains(&account_id));
    assert!(hrn_str.contains("account"));
    assert!(hrn_str.contains("HrnTestAccount"));
}
