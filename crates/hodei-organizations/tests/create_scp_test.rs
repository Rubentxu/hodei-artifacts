use hodei_organizations::features::create_scp::use_case::CreateScpUseCase;
use hodei_organizations::features::create_scp::dto::CreateScpCommand;
use hodei_organizations::features::create_scp::mocks::MockScpPersister;
use std::sync::Arc;

#[tokio::test]
async fn test_create_scp_success() {
    let mock_persister = Arc::new(MockScpPersister::new());
    let use_case = CreateScpUseCase::new(mock_persister.clone());
    
    let command = CreateScpCommand {
        name: "Test SCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };
    
    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let scp_view = result.unwrap();
    assert_eq!(scp_view.name, "Test SCP");
    assert_eq!(scp_view.document, "permit(principal, action, resource);");
    assert!(!scp_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_scp_empty_name() {
    let mock_persister = Arc::new(MockScpPersister::new());
    let use_case = CreateScpUseCase::new(mock_persister.clone());
    
    let command = CreateScpCommand {
        name: "".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };
    
    let result = use_case.execute(command).await;
    assert!(result.is_err());
    
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Invalid SCP name");
}

#[tokio::test]
async fn test_create_scp_empty_document() {
    let mock_persister = Arc::new(MockScpPersister::new());
    let use_case = CreateScpUseCase::new(mock_persister.clone());
    
    let command = CreateScpCommand {
        name: "Test SCP".to_string(),
        document: "".to_string(),
    };
    
    let result = use_case.execute(command).await;
    assert!(result.is_err());
    
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Invalid SCP document");
}

#[tokio::test]
async fn test_create_scp_persister_error() {
    let mock_persister = Arc::new(MockScpPersister::new_with_error(true));
    let use_case = CreateScpUseCase::new(mock_persister.clone());
    
    let command = CreateScpCommand {
        name: "Test SCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };
    
    let result = use_case.execute(command).await;
    assert!(result.is_err());
    
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Storage error: Mock storage error");
}