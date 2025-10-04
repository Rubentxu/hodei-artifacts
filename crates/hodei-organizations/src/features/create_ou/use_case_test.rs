use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::dto::{CreateOuCommand};
use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::mocks::MockOuPersister;

use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_ou_success() {
    // Arrange
    let mock_persister = MockOuPersister::new();
    let use_case = CreateOuUseCase::new(Arc::new(mock_persister));
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
    let mock_persister = MockOuPersister::new();
    let use_case = CreateOuUseCase::new(Arc::new(mock_persister));
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
