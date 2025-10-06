use crate::features::get_effective_scps::dto::{EffectiveScpsView, GetEffectiveScpsCommand};
use crate::features::get_effective_scps::mocks::{
    MockAccountRepositoryPort, MockOuRepositoryPort, MockScpRepositoryPort,
};
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::internal::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use kernel::Hrn;

#[tokio::test]
async fn test_get_effective_scps_for_account() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();

    // Create test entities
    let account_hrn = Hrn::new("account", "test-account");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    let scp_hrn = Hrn::new("scp", "test-scp");

    let account = Account::new(
        account_hrn.clone(),
        "TestAccount".to_string(),
        parent_ou_hrn.clone(),
    )
    .with_attached_scp(scp_hrn.clone());

    // Populate mocks
    account_repository.with_account(account);

    // Create use case
    let use_case = GetEffectiveScpsUseCase::new(scp_repository, account_repository, ou_repository);

    // Create command
    let command = GetEffectiveScpsCommand {
        target_hrn: account_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let effective_scps_view = result.unwrap();
    assert_eq!(effective_scps_view.target_hrn, account_hrn.to_string());
    assert_eq!(
        effective_scps_view.effective_scps,
        vec![scp_hrn.to_string()]
    );
}

#[tokio::test]
async fn test_get_effective_scps_for_ou() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();

    // Create test entities
    let ou_hrn = Hrn::new("ou", "test-ou");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    let scp_hrn = Hrn::new("scp", "test-scp");

    let ou = OrganizationalUnit::new(ou_hrn.clone(), "TestOU".to_string(), parent_ou_hrn.clone())
        .with_attached_scp(scp_hrn.clone());

    // Populate mocks
    ou_repository.with_ou(ou);

    // Create use case
    let use_case = GetEffectiveScpsUseCase::new(scp_repository, account_repository, ou_repository);

    // Create command
    let command = GetEffectiveScpsCommand {
        target_hrn: ou_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let effective_scps_view = result.unwrap();
    assert_eq!(effective_scps_view.target_hrn, ou_hrn.to_string());
    assert_eq!(
        effective_scps_view.effective_scps,
        vec![scp_hrn.to_string()]
    );
}
