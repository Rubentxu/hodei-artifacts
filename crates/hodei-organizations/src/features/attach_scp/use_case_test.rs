use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::use_case::AttachScpUseCase;
use crate::features::attach_scp::mocks::{MockScpRepositoryPort, MockAccountRepositoryPort, MockOuRepositoryPort};
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use policies::shared::domain::hrn::Hrn;

#[tokio::test]
async fn test_attach_scp_to_account() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let scp_hrn = Hrn::new("scp", "test-scp");
    let account_hrn = Hrn::new("account", "test-account");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    
    let scp = ServiceControlPolicy::new(
        scp_hrn.clone(),
        "TestSCP".to_string(),
        "permit(principal, action, resource);".to_string(),
    );
    
    let account = Account::new(
        account_hrn.clone(),
        "TestAccount".to_string(),
        parent_ou_hrn.clone(),
    );
    
    // Populate mocks
    scp_repository.with_scp(scp);
    account_repository.with_account(account);
    
    // Create use case
    let use_case = AttachScpUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = AttachScpCommand {
        scp_hrn: scp_hrn.to_string(),
        target_hrn: account_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let attach_view = result.unwrap();
    assert_eq!(attach_view.scp_hrn, scp_hrn.to_string());
    assert_eq!(attach_view.target_hrn, account_hrn.to_string());
}

#[tokio::test]
async fn test_attach_scp_to_ou() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let scp_hrn = Hrn::new("scp", "test-scp");
    let ou_hrn = Hrn::new("ou", "test-ou");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    
    let scp = ServiceControlPolicy::new(
        scp_hrn.clone(),
        "TestSCP".to_string(),
        "permit(principal, action, resource);".to_string(),
    );
    
    let ou = OrganizationalUnit::new(
        ou_hrn.clone(),
        "TestOU".to_string(),
        parent_ou_hrn.clone(),
    );
    
    // Populate mocks
    scp_repository.with_scp(scp);
    ou_repository.with_ou(ou);
    
    // Create use case
    let use_case = AttachScpUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = AttachScpCommand {
        scp_hrn: scp_hrn.to_string(),
        target_hrn: ou_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let attach_view = result.unwrap();
    assert_eq!(attach_view.scp_hrn, scp_hrn.to_string());
    assert_eq!(attach_view.target_hrn, ou_hrn.to_string());
}
