//! Unit tests for SurrealOrganizationBoundaryProvider
//!
//! These tests verify the SCP resolution algorithm using in-memory mock repositories.

use super::organization_boundary_provider::SurrealOrganizationBoundaryProvider;
use crate::features::evaluate_permissions::ports::OrganizationBoundaryProvider;
use async_trait::async_trait;
use hodei_organizations::internal_api::application::ports::account_repository::{
    AccountRepository, AccountRepositoryError,
};
use hodei_organizations::internal_api::application::ports::ou_repository::{
    OuRepository, OuRepositoryError,
};
use hodei_organizations::internal_api::application::ports::scp_repository::{
    ScpRepository, ScpRepositoryError,
};
use hodei_organizations::internal_api::domain::Account;
use hodei_organizations::internal_api::domain::OrganizationalUnit;
use hodei_organizations::internal_api::domain::ServiceControlPolicy;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ============================================================================
// Mock Repositories
// ============================================================================

#[derive(Clone)]
struct InMemoryScpRepository {
    scps: Arc<Mutex<HashMap<String, ServiceControlPolicy>>>,
}

impl InMemoryScpRepository {
    fn new() -> Self {
        Self {
            scps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_str = scp.hrn.to_string();
        self.scps.lock().unwrap().insert(hrn_str, scp);
        self
    }
}

#[async_trait]
impl ScpRepository for InMemoryScpRepository {
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let hrn_str = scp.hrn.to_string();
        self.scps.lock().unwrap().insert(hrn_str, scp.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let hrn_str = hrn.to_string();
        Ok(self.scps.lock().unwrap().get(&hrn_str).cloned())
    }
}

#[derive(Clone)]
struct InMemoryAccountRepository {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl InMemoryAccountRepository {
    fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_account(self, account: Account) -> Self {
        let hrn_str = account.hrn.to_string();
        self.accounts.lock().unwrap().insert(hrn_str, account);
        self
    }
}

#[async_trait]
impl AccountRepository for InMemoryAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let hrn_str = account.hrn.to_string();
        self.accounts
            .lock()
            .unwrap()
            .insert(hrn_str, account.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let hrn_str = hrn.to_string();
        Ok(self.accounts.lock().unwrap().get(&hrn_str).cloned())
    }
}

#[derive(Clone)]
struct InMemoryOuRepository {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
}

impl InMemoryOuRepository {
    fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_ou(self, ou: OrganizationalUnit) -> Self {
        let hrn_str = ou.hrn.to_string();
        self.ous.lock().unwrap().insert(hrn_str, ou);
        self
    }
}

#[async_trait]
impl OuRepository for InMemoryOuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        self.ous.lock().unwrap().insert(hrn_str, ou.clone());
        Ok(())
    }

    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let hrn_str = hrn.to_string();
        Ok(self.ous.lock().unwrap().get(&hrn_str).cloned())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_scp(id: &str, policy_text: &str) -> ServiceControlPolicy {
    let hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "ServiceControlPolicy".to_string(),
        id.to_string(),
    );

    ServiceControlPolicy {
        hrn,
        name: format!("SCP-{}", id),
        document: policy_text.to_string(),
    }
}

fn create_test_account(id: &str, parent_ou_hrn: Option<Hrn>) -> Account {
    let hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "Account".to_string(),
        id.to_string(),
    );

    Account::new(hrn, format!("Account-{}", id), parent_ou_hrn)
}

fn create_test_ou(id: &str, parent_hrn: Hrn) -> OrganizationalUnit {
    let hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        id.to_string(),
    );

    OrganizationalUnit {
        hrn,
        name: format!("OU-{}", id),
        parent_hrn,
        child_ous: Default::default(),
        child_accounts: Default::default(),
        attached_scps: Default::default(),
    }
}

fn simple_cedar_policy(id: &str) -> String {
    format!(
        r#"permit(principal, action, resource) when {{ resource.type == "{}" }};"#,
        id
    )
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_account_with_single_level_hierarchy() {
    // Arrange: Account → OU (with SCP)
    let root_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "root".to_string(),
    );

    let scp1 = create_test_scp("scp-1", &simple_cedar_policy("test1"));
    let scp1_hrn = scp1.hrn.clone();

    let mut root_ou = create_test_ou("root", root_hrn.clone()); // Root points to itself
    root_ou.attach_scp(scp1_hrn.clone());

    let mut account = create_test_account("acc-1", Some(root_hrn.clone()));
    account.attach_scp(scp1_hrn.clone());

    let scp_repo = InMemoryScpRepository::new().with_scp(scp1);
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new().with_ou(root_ou);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    assert_eq!(policy_set.policies().count(), 1);
}

#[tokio::test]
async fn test_account_with_deep_hierarchy() {
    // Arrange: Account → OU3 → OU2 → OU1 → Root
    let root_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "root".to_string(),
    );

    let scp1 = create_test_scp("scp-1", &simple_cedar_policy("level1"));
    let scp2 = create_test_scp("scp-2", &simple_cedar_policy("level2"));
    let scp3 = create_test_scp("scp-3", &simple_cedar_policy("level3"));
    let scp4 = create_test_scp("scp-4", &simple_cedar_policy("root"));

    let scp1_hrn = scp1.hrn.clone();
    let scp2_hrn = scp2.hrn.clone();
    let scp3_hrn = scp3.hrn.clone();
    let scp4_hrn = scp4.hrn.clone();

    // Root OU (points to itself)
    let mut root_ou = create_test_ou("root", root_hrn.clone());
    root_ou.attach_scp(scp4_hrn.clone());

    // Level 1 OU
    let ou1_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "ou-1".to_string(),
    );
    let mut ou1 = create_test_ou("ou-1", root_hrn.clone());
    ou1.attach_scp(scp1_hrn.clone());

    // Level 2 OU
    let ou2_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "ou-2".to_string(),
    );
    let mut ou2 = create_test_ou("ou-2", ou1_hrn.clone());
    ou2.attach_scp(scp2_hrn.clone());

    // Level 3 OU
    let ou3_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "ou-3".to_string(),
    );
    let mut ou3 = create_test_ou("ou-3", ou2_hrn.clone());
    ou3.attach_scp(scp3_hrn.clone());

    // Account in OU3
    let account = create_test_account("acc-deep", Some(ou3_hrn.clone()));

    let scp_repo = InMemoryScpRepository::new()
        .with_scp(scp1)
        .with_scp(scp2)
        .with_scp(scp3)
        .with_scp(scp4);
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new()
        .with_ou(root_ou)
        .with_ou(ou1)
        .with_ou(ou2)
        .with_ou(ou3);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    // Should have SCPs from OU3, OU2, OU1, and Root
    assert_eq!(policy_set.policies().count(), 4);
}

#[tokio::test]
async fn test_account_without_parent() {
    // Arrange: Orphan account with no parent OU
    let scp1 = create_test_scp("scp-orphan", &simple_cedar_policy("orphan"));
    let scp1_hrn = scp1.hrn.clone();

    let mut account = create_test_account("acc-orphan", None);
    account.attach_scp(scp1_hrn.clone());

    let scp_repo = InMemoryScpRepository::new().with_scp(scp1);
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new();

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    // Only the account's own SCP, no hierarchy
    assert_eq!(policy_set.policies().count(), 1);
}

#[tokio::test]
async fn test_ou_without_scps() {
    // Arrange: OU with no attached SCPs, but parent has SCP
    let root_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "root".to_string(),
    );

    let scp1 = create_test_scp("scp-root", &simple_cedar_policy("root"));
    let scp1_hrn = scp1.hrn.clone();

    let mut root_ou = create_test_ou("root", root_hrn.clone());
    root_ou.attach_scp(scp1_hrn.clone());

    let empty_ou_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "empty-ou".to_string(),
    );
    let empty_ou = create_test_ou("empty-ou", root_hrn.clone());
    // No SCPs attached to empty_ou

    let scp_repo = InMemoryScpRepository::new().with_scp(scp1);
    let account_repo = InMemoryAccountRepository::new();
    let ou_repo = InMemoryOuRepository::new()
        .with_ou(root_ou)
        .with_ou(empty_ou);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&empty_ou_hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    // Should only have the root SCP
    assert_eq!(policy_set.policies().count(), 1);
}

#[tokio::test]
async fn test_malformed_scp_is_skipped() {
    // Arrange: Account with one valid and one malformed SCP
    let root_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "root".to_string(),
    );

    let valid_scp = create_test_scp("scp-valid", &simple_cedar_policy("valid"));
    let malformed_scp = create_test_scp("scp-bad", "this is not valid Cedar syntax!!!");

    let valid_hrn = valid_scp.hrn.clone();
    let malformed_hrn = malformed_scp.hrn.clone();

    let mut root_ou = create_test_ou("root", root_hrn.clone());
    root_ou.attach_scp(valid_hrn.clone());
    root_ou.attach_scp(malformed_hrn.clone());

    let account = create_test_account("acc-mixed", Some(root_hrn.clone()));

    let scp_repo = InMemoryScpRepository::new()
        .with_scp(valid_scp)
        .with_scp(malformed_scp);
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new().with_ou(root_ou);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    // Only the valid SCP should be in the set (malformed is skipped with warning)
    assert_eq!(policy_set.policies().count(), 1);
}

#[tokio::test]
async fn test_missing_scp_reference() {
    // Arrange: OU references an SCP that doesn't exist in repository
    let root_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "root".to_string(),
    );

    let existing_scp = create_test_scp("scp-exists", &simple_cedar_policy("exists"));
    let existing_hrn = existing_scp.hrn.clone();

    let missing_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "ServiceControlPolicy".to_string(),
        "scp-missing".to_string(),
    );

    let mut root_ou = create_test_ou("root", root_hrn.clone());
    root_ou.attach_scp(existing_hrn.clone());
    root_ou.attach_scp(missing_hrn); // This one doesn't exist

    let account = create_test_account("acc-test", Some(root_hrn.clone()));

    let scp_repo = InMemoryScpRepository::new().with_scp(existing_scp);
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new().with_ou(root_ou);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_ok());
    let policy_set = result.unwrap();
    // Only the existing SCP (missing one is logged as warning)
    assert_eq!(policy_set.policies().count(), 1);
}

#[tokio::test]
async fn test_account_not_found() {
    // Arrange: Try to get SCPs for non-existent account
    let nonexistent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "Account".to_string(),
        "does-not-exist".to_string(),
    );

    let scp_repo = InMemoryScpRepository::new();
    let account_repo = InMemoryAccountRepository::new();
    let ou_repo = InMemoryOuRepository::new();

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&nonexistent_hrn).await;

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Account not found"));
}

#[tokio::test]
async fn test_ou_not_found() {
    // Arrange: Try to get SCPs for non-existent OU
    let nonexistent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "does-not-exist".to_string(),
    );

    let scp_repo = InMemoryScpRepository::new();
    let account_repo = InMemoryAccountRepository::new();
    let ou_repo = InMemoryOuRepository::new();

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&nonexistent_hrn).await;

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Organizational Unit not found"));
}

#[tokio::test]
async fn test_invalid_resource_type() {
    // Arrange: HRN with invalid resource type
    let invalid_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "InvalidType".to_string(),
        "test".to_string(),
    );

    let scp_repo = InMemoryScpRepository::new();
    let account_repo = InMemoryAccountRepository::new();
    let ou_repo = InMemoryOuRepository::new();

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&invalid_hrn).await;

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid target type"));
}

#[tokio::test]
async fn test_cycle_detection_in_ou_hierarchy() {
    // Arrange: Create a cycle OU1 → OU2 → OU1
    let ou1_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "ou-1".to_string(),
    );

    let ou2_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "default".to_string(),
        "OrganizationalUnit".to_string(),
        "ou-2".to_string(),
    );

    // OU1 points to OU2
    let ou1 = create_test_ou("ou-1", ou2_hrn.clone());

    // OU2 points back to OU1 (cycle!)
    let ou2 = create_test_ou("ou-2", ou1_hrn.clone());

    let account = create_test_account("acc-cycle", Some(ou1_hrn.clone()));

    let scp_repo = InMemoryScpRepository::new();
    let account_repo = InMemoryAccountRepository::new().with_account(account.clone());
    let ou_repo = InMemoryOuRepository::new().with_ou(ou1).with_ou(ou2);

    let provider = SurrealOrganizationBoundaryProvider::new(scp_repo, account_repo, ou_repo);

    // Act
    let result = provider.get_effective_scps_for(&account.hrn).await;

    // Assert
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cycle detected"));
}
