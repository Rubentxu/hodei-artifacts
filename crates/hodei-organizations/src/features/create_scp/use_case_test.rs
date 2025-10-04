use crate::features::create_scp::dto::CreateScpCommand;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::mocks::MockScpPersister;
use crate::features::create_scp::dto::ScpView;

#[tokio::test]
async fn test_create_scp_use_case() {
    // Arrange
    let persister = MockScpPersister::new();
    let use_case = CreateScpUseCase::new(std::sync::Arc::new(persister));
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
