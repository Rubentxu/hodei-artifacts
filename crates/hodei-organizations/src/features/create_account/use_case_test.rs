use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::features::create_account::dto::{CreateAccountCommand};
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::mocks::MockAccountPersister;

use std::sync::Arc;
use policies::shared::domain::hrn::Hrn;

#[tokio::test]
async fn test_create_account_success() {
    // Arrange
    let mock_persister = MockAccountPersister::new();
    let use_case = CreateAccountUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: parent_hrn.clone(),
        hrn: Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "123456789012".to_string(),
            "account".to_string(),
            "test-account".to_string(),
        ),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, parent_hrn);
    assert!(!account_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_account_empty_name() {
    // Arrange
    let mock_persister = MockAccountPersister::new();
    let use_case = CreateAccountUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "".to_string(),
        parent_hrn,
        hrn: Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "123456789012".to_string(),
            "account".to_string(),
            "test-account".to_string(),
        ),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateAccountError::InvalidAccountName));
}
