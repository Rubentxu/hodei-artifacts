This file is a merged representation of a subset of the codebase, containing specifically included files, combined into a single document by Repomix.

<file_summary>
This section contains a summary of this file.

<purpose>
This file contains a packed representation of a subset of the repository's contents that is considered the most important context.
It is designed to be easily consumable by AI systems for analysis, code review,
or other automated processes.
</purpose>

<file_format>
The content is organized as follows:
1. This summary section
2. Repository information
3. Directory structure
4. Repository files (if enabled)
5. Multiple file entries, each consisting of:
  - File path as an attribute
  - Full contents of the file
</file_format>

<usage_guidelines>
- This file should be treated as read-only. Any changes should be made to the
  original repository files, not this packed version.
- When processing this file, use the file path to distinguish
  between different files in the repository.
- Be aware that this file may contain sensitive information. Handle it with
  the same level of security as you would the original repository.
</usage_guidelines>

<notes>
- Some files may have been excluded based on .gitignore rules and Repomix's configuration
- Binary files are not included in this packed representation. Please refer to the Repository Structure section for a complete list of file paths, including binary files
- Only files matching these patterns are included: crates/policies/**/*, crates/hodei-iam/**/*, crates/hodei-organizations/**/*
- Files matching patterns in .gitignore are excluded
- Files matching default ignore patterns are excluded
- Files are sorted by Git change count (files with more changes are at the bottom)
</notes>

</file_summary>

<directory_structure>
crates/
  hodei-iam/
    src/
      features/
        add_user_to_group/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_group/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_user/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        mod.rs
      shared/
        application/
          ports/
            mod.rs
          di_configurator.rs
          mod.rs
        domain/
          actions.rs
          entities.rs
          mod.rs
        infrastructure/
          persistence/
            mod.rs
          surreal/
            iam_policy_provider.rs
          mod.rs
        mod.rs
      lib.rs
    tests/
      add_user_to_group_integration_test.rs
      create_user_integration_test.rs
      integration_add_user_to_group_comprehensive_test.rs
      integration_create_user_comprehensive_test.rs
      unit_group_test.rs
      unit_hrn_constructor_test.rs
      unit_user_test.rs
    Cargo.toml
  hodei-organizations/
    src/
      features/
        attach_scp/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_account/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_ou/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_scp/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        get_effective_scps/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        move_account/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        mod.rs
      shared/
        application/
          ports/
            account_repository.rs
            mod.rs
            ou_repository.rs
            scp_repository.rs
          hierarchy_service.rs
          mod.rs
        domain/
          account.rs
          mod.rs
          ou_test.rs
          ou.rs
          scp_test.rs
          scp.rs
        infrastructure/
          surreal/
            account_repository.rs
            organization_boundary_provider.rs
            ou_repository.rs
            scp_repository.rs
        mod.rs
      lib.rs
    tests/
      attach_scp_test.rs
      create_account_test.rs
      create_ou_test.rs
      create_scp_test.rs
      get_effective_scps_test.rs
      move_account_test.rs
    Cargo.toml
    README.md
  policies/
    src/
      features/
        batch_eval/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        delete_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        get_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        list_policies/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_analysis/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_playground/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_playground_traces/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        update_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        validate_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        mod.rs
      shared/
        application/
          di_helpers.rs
          engine.rs
          mod.rs
          parallel.rs
          store.rs
        domain/
          entity_utils.rs
          error.rs
          hrn.rs
          mod.rs
          ports.rs
          schema_assembler.rs
        infrastructure/
          surreal/
            embedded_storage.rs
            mem_storage.rs
            mod.rs
          mod.rs
        mod.rs
      lib.rs
    tests/
      delete_policy_integration_test.rs
      domain_compilation_test.rs
      hodei_entity_test.rs
      list_policies_integration_test.rs
      principals_schema_test.rs
      schema_rendering_final_test.rs
      shared_parallel_test.rs
      test_schema.rs
    Cargo.toml
</directory_structure>

<files>
This section contains the contents of the repository's files.

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/iam_policy_provider.rs">
use hodei_authorizer::ports::{IamPolicyProvider, AuthorizationError};
use hodei_authorizer::ports::PolicySet;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;

/// SurrealDB implementation of IamPolicyProvider
pub struct SurrealIamPolicyProvider {
    db: Surreal<Any>,
}

impl SurrealIamPolicyProvider {
    /// Create a new SurrealIamPolicyProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl IamPolicyProvider for SurrealIamPolicyProvider {
    /// Get identity policies for a principal
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError> {
        // TODO: Implement the actual logic to retrieve IAM policies for a principal
        // This would involve querying the SurrealDB database for policies associated with the principal
        
        // For now, we'll return an empty policy set
        Ok(PolicySet::new())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/adapter.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;
use async_trait::async_trait;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository + std::marker::Send> {
    repository: SR,
}

impl<SR: ScpRepository + std::marker::Send> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository + std::marker::Sync + std::marker::Send> ScpRepositoryPort for ScpRepositoryAdapter<SR> {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository + std::marker::Send> {
    repository: AR,
}

impl<AR: AccountRepository + std::marker::Send> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + std::marker::Sync + std::marker::Send> AccountRepositoryPort for AccountRepositoryAdapter<AR> {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
    
    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        self.repository.save(&account).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository + std::marker::Send> {
    repository: OR,
}

impl<OR: OuRepository + std::marker::Send> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository + std::marker::Sync + std::marker::Send> OuRepositoryPort for OuRepositoryAdapter<OR> {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
    
    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        self.repository.save(&ou).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/di.rs">
use crate::shared::application::ports::scp_repository::ScpRepository;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::features::attach_scp::use_case::AttachScpUseCase;
use crate::features::attach_scp::adapter::{ScpRepositoryAdapter, AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the AttachScpUseCase with the provided repositories
pub fn attach_scp_use_case<SR: ScpRepository + std::marker::Sync + std::marker::Send, AR: AccountRepository + std::marker::Sync + std::marker::Send, OR: OuRepository + std::marker::Sync + std::marker::Send>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> AttachScpUseCase<ScpRepositoryAdapter<SR>, AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    AttachScpUseCase::new(scp_adapter, account_adapter, ou_adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/dto.rs">
use serde::{Deserialize, Serialize};

/// Command to attach an SCP to an entity (Account or OU)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpCommand {
    /// HRN of the SCP to attach
    pub scp_hrn: String,
    /// HRN of the target entity (Account or OU)
    pub target_hrn: String,
}

/// View of the attach SCP operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpView {
    /// HRN of the SCP that was attached
    pub scp_hrn: String,
    /// HRN of the target entity
    pub target_hrn: String,
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/error.rs">
use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

/// Error type for attach SCP use case
#[derive(Debug, Error)]
pub enum AttachScpError {
    #[error("SCP repository error: {0}")]
    ScpRepository(#[from] ScpRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("SCP not found: {0}")]
    ScpNotFound(String),
    #[error("Target entity not found: {0}")]
    TargetNotFound(String),
    #[error("Invalid target entity type: {0}")]
    InvalidTargetType(String),
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/mocks.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;

use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of ScpRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockScpRepositoryPort {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepositoryPort {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepositoryPort for MockScpRepositoryPort {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of AccountRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockAccountRepositoryPort {
    accounts: RwLock<HashMap<String, Account>>,
}

impl MockAccountRepositoryPort {
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_account(self, account: Account) -> Self {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
        self
    }

    pub fn update_account(&self, account: Account) {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
    }
}

#[async_trait]
impl AccountRepositoryPort for MockAccountRepositoryPort {
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let accounts = self.accounts.read().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }

    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

/// Mock implementation of OuRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockOuRepositoryPort {
    ous: RwLock<HashMap<String, OrganizationalUnit>>,
}

impl MockOuRepositoryPort {
    pub fn new() -> Self {
        Self {
            ous: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_ou(self, ou: OrganizationalUnit) -> Self {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
        self
    }

    pub fn update_ou(&self, ou: OrganizationalUnit) {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
    }
}

#[async_trait]
impl OuRepositoryPort for MockOuRepositoryPort {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let ous = self.ous.read().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }

    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let mut ous = self.ous.write().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/mod.rs">
pub mod use_case;
pub mod dto;
pub mod error;
pub mod ports;
pub mod adapter;
pub mod di;
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/ports.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use policies::domain::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving and updating accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
    
    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError>;
}

/// Port for retrieving and updating organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
    
    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/use_case_test.rs">
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
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/use_case.rs">
use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::error::AttachScpError;
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;

/// Use case for attaching an SCP to an entity (Account or OU)
pub struct AttachScpUseCase<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> {
    scp_repository: SRP,
    account_repository: ARP,
    ou_repository: ORP,
}

impl<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> AttachScpUseCase<SRP, ARP, ORP> {
    /// Create a new instance of the use case
    pub fn new(scp_repository: SRP, account_repository: ARP, ou_repository: ORP) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
        }
    }

    /// Execute the use case
    pub async fn execute(&self, command: AttachScpCommand) -> Result<AttachScpView, AttachScpError> {
        // Parse HRNs
        let scp_hrn = Hrn::from_string(&command.scp_hrn)
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;
        let target_hrn = Hrn::from_string(&command.target_hrn)
            .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;

        // Find the SCP
        let _scp = self.scp_repository.find_scp_by_hrn(&scp_hrn).await?
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;

        // Attach SCP based on target entity type
        match target_hrn.resource_type.as_str() {
            "account" => {
                let mut account = self.account_repository.find_account_by_hrn(&target_hrn).await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                account.attach_scp(scp_hrn.clone());
                self.account_repository.save_account(account).await?;
            },
            "ou" => {
                let mut ou = self.ou_repository.find_ou_by_hrn(&target_hrn).await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                ou.attach_scp(scp_hrn.clone());
                self.ou_repository.save_ou(ou).await?;
            },
            _ => return Err(AttachScpError::InvalidTargetType(target_hrn.resource_type.clone())),
        }

        // Return the attach SCP view
        Ok(AttachScpView {
            scp_hrn: scp_hrn.to_string(),
            target_hrn: target_hrn.to_string(),
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/adapter.rs">
use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::domain::account::Account;
use crate::shared::application::ports::account_repository::AccountRepository;
use async_trait::async_trait;
use std::sync::Arc;

pub struct AccountPersisterAdapter<AR: AccountRepository> {
    repository: Arc<AR>,
}

impl<AR: AccountRepository> AccountPersisterAdapter<AR> {
    pub fn new(repository: Arc<AR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository> AccountPersister for AccountPersisterAdapter<AR> {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        self.repository.save(&account).await?;
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/di.rs">
use crate::shared::application::ports::AccountRepository;
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::features::create_account::adapter::AccountRepositoryAdapter;

/// Create an instance of the CreateAccountUseCase with the provided repository
pub fn create_account_use_case<AR: AccountRepository>(
    account_repository: AR,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountCommand {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/error.rs">
use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;

#[derive(Debug, Error)]
pub enum CreateAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("Invalid account name")]
    InvalidAccountName,
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/mocks.rs">
use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockAccountPersister {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl MockAccountPersister {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AccountPersister for MockAccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
mod adapter;
</file>

<file path="crates/hodei-organizations/src/features/create_account/ports.rs">
use crate::shared::domain::account::Account;
use crate::features::create_account::error::CreateAccountError;
use async_trait::async_trait;

#[async_trait]
pub trait AccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/use_case_test.rs">
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::features::create_account::dto::{CreateAccountCommand, AccountView};
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::mocks::MockAccountPersister;

use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_account_success() {
    // Arrange
    let mock_persister = MockAccountPersister::new();
    let use_case = CreateAccountUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::new(
        "default",
     "");
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: parent_hrn.clone(),
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
    let parent_hrn = Hrn::generate("ou");
    let command = CreateAccountCommand {
        name: "".to_string(),
        parent_hrn,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateAccountError::InvalidAccountName));
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/use_case.rs">
use crate::shared::domain::account::Account;
use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::dto::{CreateAccountCommand, AccountView};
use crate::features::create_account::error::CreateAccountError;
use std::sync::Arc;

pub struct CreateAccountUseCase<AP: AccountPersister> {
    persister: Arc<AP>,
}

impl<AP: AccountPersister> CreateAccountUseCase<AP> {
    pub fn new(persister: Arc<AP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateAccountCommand) -> Result<AccountView, CreateAccountError> {
        // Validar el nombre de la cuenta
        if command.name.is_empty() {
            return Err(CreateAccountError::InvalidAccountName);
        }
        
        // Crear la cuenta
        let account = Account::new(command.hrn, command.name.clone(), command.parent_hrn);
        
        // Guardar la cuenta
        self.persister.save(account.clone()).await?;
        
        // Devolver la vista de la cuenta
        Ok(AccountView {
            hrn: account.hrn,
            name: account.name,
            parent_hrn: account.parent_hrn,
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/adapter.rs">
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::ou_repository::OuRepository;
use async_trait::async_trait;
use std::sync::Arc;

pub struct OuPersisterAdapter<OR: OuRepository> {
    repository: Arc<OR>,
}

impl<OR: OuRepository> OuPersisterAdapter<OR> {
    pub fn new(repository: Arc<OR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> OuPersister for OuPersisterAdapter<OR> {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        self.repository.save(&ou).await?;
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/di.rs">
use crate::shared::application::ports::OuRepository;
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::adapter::OuRepositoryAdapter;

/// Create an instance of the CreateOuUseCase with the provided repository
pub fn create_ou_use_case<OR: OuRepository>(
    ou_repository: OR,
) -> CreateOuUseCase<OuRepositoryAdapter<OR>> {
    let adapter = OuRepositoryAdapter::new(ou_repository);
    CreateOuUseCase::new(adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOuCommand {
    pub name: String,
    pub parent_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OuView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/error.rs">
use thiserror::Error;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum CreateOuError {
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Invalid OU name")]
    InvalidOuName,
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/mocks.rs">
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockOuPersister {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
}

impl MockOuPersister {
    pub fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl OuPersister for MockOuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/create_ou/ports.rs">
use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::create_ou::error::CreateOuError;
use async_trait::async_trait;

#[async_trait]
pub trait OuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/use_case_test.rs">
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::dto::{CreateOuCommand, OuView};
use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::mocks::MockOuPersister;
use shared::domain::hrn::Hrn;
use std::sync::Arc;

#[tokio::test]
async fn test_create_ou_success() {
    // Arrange
    let mock_persister = MockOuPersister::new();
    let use_case = CreateOuUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::generate("ou");
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
    let parent_hrn = Hrn::generate("ou");
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
</file>

<file path="crates/hodei-organizations/src/features/create_ou/use_case.rs">
use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::dto::{CreateOuCommand, OuView};
use crate::features::create_ou::error::CreateOuError;
use std::sync::Arc;

pub struct CreateOuUseCase<OP: OuPersister> {
    persister: Arc<OP>,
}

impl<OP: OuPersister> CreateOuUseCase<OP> {
    pub fn new(persister: Arc<OP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateOuCommand) -> Result<OuView, CreateOuError> {
        // Validar el nombre de la OU
        if command.name.is_empty() {
            return Err(CreateOuError::InvalidOuName);
        }
        
        // Crear la OU
        let ou = OrganizationalUnit::new(command.name.clone(), command.parent_hrn.clone());
        
        // Guardar la OU
        self.persister.save(ou.clone()).await?;
        
        // Devolver la vista de la OU
        Ok(OuView {
            hrn: ou.hrn,
            name: ou.name,
            parent_hrn: ou.parent_hrn,
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/adapter.rs">
use crate::shared::domain::ServiceControlPolicy;
use crate::shared::application::ports::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use async_trait::async_trait;

/// Adapter that implements the ScpPersister trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpPersister for ScpRepositoryAdapter<SR> {
    /// Save an SCP using the repository
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        self.repository.save(&scp).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/di.rs">
use crate::shared::application::ports::ScpRepository;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::adapter::ScpRepositoryAdapter;

/// Create an instance of the CreateScpUseCase with the provided repository
pub fn create_scp_use_case<SR: ScpRepository>(
    scp_repository: SR,
) -> CreateScpUseCase<ScpRepositoryAdapter<SR>> {
    let adapter = ScpRepositoryAdapter::new(scp_repository);
    CreateScpUseCase::new(adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScpCommand {
    pub name: String,
    pub document: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpView {
    pub hrn: Hrn,
    pub name: String,
    pub document: String,
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/error.rs">
use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;

#[derive(Debug, Error)]
pub enum CreateScpError {
    #[error("SCP repository error: {0}")]
    ScpRepositoryError(#[from] ScpRepositoryError),
    #[error("Invalid SCP name")]
    InvalidScpName,
    #[error("Invalid SCP document")]
    InvalidScpDocument,
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/mocks.rs">
use crate::shared::domain::ServiceControlPolicy;
use crate::shared::application::ports::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of ScpRepository for testing
#[derive(Debug, Default)]
pub struct MockScpRepository {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepository {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(mut self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepository for MockScpRepository {
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let mut scps = self.scps.write().unwrap();
        scps.insert(scp.hrn.to_string(), scp.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of ScpPersister for testing
#[derive(Debug, Default)]
pub struct MockScpPersister {
    saved_scps: RwLock<Vec<ServiceControlPolicy>>,
}

impl MockScpPersister {
    pub fn new() -> Self {
        Self {
            saved_scps: RwLock::new(Vec::new()),
        }
    }

    pub fn get_saved_scps(&self) -> Vec<ServiceControlPolicy> {
        self.saved_scps.read().unwrap().clone()
    }
}

#[async_trait]
impl ScpPersister for MockScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let mut saved_scps = self.saved_scps.write().unwrap();
        saved_scps.push(scp);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/create_scp/ports.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::create_scp::error::CreateScpError;
use async_trait::async_trait;

#[async_trait]
pub trait ScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/use_case_test.rs">
use crate::features::create_scp::dto::CreateScpCommand;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::mocks::MockScpPersister;

#[tokio::test]
async fn test_create_scp_use_case() {
    // Arrange
    let persister = MockScpPersister::new();
    let use_case = CreateScpUseCase::new(persister);
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
    assert!(scp_view.hrn.starts_with("hrn:scp:"));
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/use_case.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::dto::{CreateScpCommand, ScpView};
use crate::features::create_scp::error::CreateScpError;
use policies::domain::Hrn;
use std::sync::Arc;

pub struct CreateScpUseCase<SP: ScpPersister> {
    persister: Arc<SP>,
}

impl<SP: ScpPersister> CreateScpUseCase<SP> {
    pub fn new(persister: Arc<SP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateScpCommand) -> Result<ScpView, CreateScpError> {
        // Validar el nombre de la SCP
        if command.name.is_empty() {
            return Err(CreateScpError::InvalidScpName);
        }
        
        // Validar el documento de la SCP
        if command.document.is_empty() {
            return Err(CreateScpError::InvalidScpDocument);
        }
        
        // Crear el HRN para la SCP
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            command.name.clone(),
        );

        // Crear la SCP
        let scp = ServiceControlPolicy::new(scp_hrn, command.name.clone(), command.document.clone());
        
        // Guardar la SCP
        self.persister.save(scp.clone()).await?;
        
        // Devolver la vista de la SCP
        Ok(ScpView {
            hrn: scp.hrn,
            name: scp.name,
            document: scp.document,
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/adapter.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::shared::application::ports::{ScpRepository, AccountRepository, OuRepository, ScpRepositoryError, AccountRepositoryError, OuRepositoryError};
use crate::features::get_effective_scps::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::shared::domain::hrn::Hrn;
use async_trait::async_trait;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpRepositoryPort for ScpRepositoryAdapter<SR> {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: AR,
}

impl<AR: AccountRepository> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository> AccountRepositoryPort for AccountRepositoryAdapter<AR> {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository> {
    repository: OR,
}

impl<OR: OuRepository> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> OuRepositoryPort for OuRepositoryAdapter<OR> {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/di.rs">
use crate::shared::application::ports::{ScpRepository, AccountRepository, OuRepository};
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::adapter::{ScpRepositoryAdapter, AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the GetEffectiveScpsUseCase with the provided repositories
pub fn get_effective_scps_use_case<SR: ScpRepository, AR: AccountRepository, OR: OuRepository>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> GetEffectiveScpsUseCase<ScpRepositoryAdapter<SR>, AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    GetEffectiveScpsUseCase::new(scp_adapter, account_adapter, ou_adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/dto.rs">
use serde::{Deserialize, Serialize};

/// Command to get effective SCPs for an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectiveScpsCommand {
    /// HRN of the target entity (Account or OU)
    pub target_hrn: String,
}

/// View of effective SCPs for an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveScpsView {
    /// HRN of the target entity
    pub target_hrn: String,
    /// List of effective SCP HRNs
    pub effective_scps: Vec<String>,
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/error.rs">
use thiserror::Error;
use crate::shared::application::ports::{ScpRepositoryError, AccountRepositoryError, OuRepositoryError};

/// Error type for get effective SCPs use case
#[derive(Debug, Error)]
pub enum GetEffectiveScpsError {
    #[error("SCP repository error: {0}")]
    ScpRepository(#[from] ScpRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("Target entity not found: {0}")]
    TargetNotFound(String),
    #[error("Invalid target entity type: {0}")]
    InvalidTargetType(String),
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/mocks.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::shared::application::ports::{ScpRepositoryError, AccountRepositoryError, OuRepositoryError};
use crate::features::get_effective_scps::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of ScpRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockScpRepositoryPort {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepositoryPort {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(mut self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepositoryPort for MockScpRepositoryPort {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of AccountRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockAccountRepositoryPort {
    accounts: RwLock<HashMap<String, Account>>,
}

impl MockAccountRepositoryPort {
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_account(mut self, account: Account) -> Self {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
        self
    }
}

#[async_trait]
impl AccountRepositoryPort for MockAccountRepositoryPort {
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let accounts = self.accounts.read().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of OuRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockOuRepositoryPort {
    ous: RwLock<HashMap<String, OrganizationalUnit>>,
}

impl MockOuRepositoryPort {
    pub fn new() -> Self {
        Self {
            ous: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_ou(mut self, ou: OrganizationalUnit) -> Self {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
        self
    }
}

#[async_trait]
impl OuRepositoryPort for MockOuRepositoryPort {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let ous = self.ous.read().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/mod.rs">
pub mod use_case;
pub mod dto;
pub mod error;
pub mod ports;
pub mod adapter;
pub mod di;
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/ports.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::shared::application::ports::{ScpRepositoryError, AccountRepositoryError, OuRepositoryError};
use policies::shared::domain::hrn::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
}

/// Port for retrieving organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/use_case_test.rs">
use crate::features::get_effective_scps::dto::{GetEffectiveScpsCommand, EffectiveScpsView};
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::mocks::{MockScpRepositoryPort, MockAccountRepositoryPort, MockOuRepositoryPort};
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use policies::shared::domain::hrn::Hrn;

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
    ).with_attached_scp(scp_hrn.clone());
    
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
    assert_eq!(effective_scps_view.effective_scps, vec![scp_hrn.to_string()]);
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
    
    let ou = OrganizationalUnit::new(
        ou_hrn.clone(),
        "TestOU".to_string(),
        parent_ou_hrn.clone(),
    ).with_attached_scp(scp_hrn.clone());
    
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
    assert_eq!(effective_scps_view.effective_scps, vec![scp_hrn.to_string()]);
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/use_case.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::features::get_effective_scps::dto::{GetEffectiveScpsCommand, EffectiveScpsView};
use crate::features::get_effective_scps::error::GetEffectiveScpsError;
use crate::features::get_effective_scps::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::shared::domain::hrn::Hrn;

/// Use case for getting effective SCPs for an entity
pub struct GetEffectiveScpsUseCase<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> {
    scp_repository: SRP,
    account_repository: ARP,
    ou_repository: ORP,
}

impl<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort> GetEffectiveScpsUseCase<SRP, ARP, ORP> {
    /// Create a new instance of the use case
    pub fn new(scp_repository: SRP, account_repository: ARP, ou_repository: ORP) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
        }
    }

    /// Execute the use case
    pub async fn execute(&self, command: GetEffectiveScpsCommand) -> Result<EffectiveScpsView, GetEffectiveScpsError> {
        // Parse target HRN
        let target_hrn = Hrn::from_str(&command.target_hrn)
            .map_err(|_| GetEffectiveScpsError::TargetNotFound(command.target_hrn.clone()))?;

        // Get effective SCPs based on target entity type
        let effective_scps = match target_hrn.entity_type.as_str() {
            "account" => {
                let account = self.account_repository.find_account_by_hrn(&target_hrn).await?
                    .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(command.target_hrn.clone()))?;
                account.attached_scps.clone()
            },
            "ou" => {
                let ou = self.ou_repository.find_ou_by_hrn(&target_hrn).await?
                    .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(command.target_hrn.clone()))?;
                ou.attached_scps.clone()
            },
            _ => return Err(GetEffectiveScpsError::InvalidTargetType(target_hrn.entity_type.clone())),
        };

        // Return the effective SCPs view
        Ok(EffectiveScpsView {
            target_hrn: target_hrn.to_string(),
            effective_scps,
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/adapter.rs">
use crate::features::move_account::ports::{AccountRepository as MoveAccountRepository, OuRepository as MoveOuRepository};
use crate::features::move_account::error::MoveAccountError;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use shared::domain::hrn::Hrn;
use async_trait::async_trait;
use std::sync::Arc;

pub struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: Arc<AR>,
}

impl<AR: AccountRepository> AccountRepositoryAdapter<AR> {
    pub fn new(repository: Arc<AR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository> MoveAccountRepository for AccountRepositoryAdapter<AR> {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, MoveAccountError> {
        self.repository.find_by_hrn(hrn).await.map_err(|e| e.into())
    }
    
    async fn save(&self, account: Account) -> Result<(), MoveAccountError> {
        self.repository.save(&account).await.map_err(|e| e.into())
    }
}

pub struct OuRepositoryAdapter<OR: OuRepository> {
    repository: Arc<OR>,
}

impl<OR: OuRepository> OuRepositoryAdapter<OR> {
    pub fn new(repository: Arc<OR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> MoveOuRepository for OuRepositoryAdapter<OR> {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, MoveAccountError> {
        self.repository.find_by_hrn(hrn).await.map_err(|e| e.into())
    }
    
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), MoveAccountError> {
        self.repository.save(&ou).await.map_err(|e| e.into())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/di.rs">
use crate::shared::application::ports::{AccountRepository, OuRepository};
use crate::features::move_account::use_case::MoveAccountUseCase;
use crate::features::move_account::adapter::{AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the MoveAccountUseCase with the provided repositories
pub fn move_account_use_case<AR: AccountRepository, OR: OuRepository>(
    account_repository: AR,
    ou_repository: OR,
) -> MoveAccountUseCase<AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    MoveAccountUseCase::new(account_adapter, ou_adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAccountCommand {
    pub account_hrn: Hrn,
    pub source_ou_hrn: Hrn,
    pub target_ou_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/error.rs">
use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum MoveAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Account not found")]
    AccountNotFound,
    #[error("Source OU not found")]
    SourceOuNotFound,
    #[error("Target OU not found")]
    TargetOuNotFound,
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/mocks.rs">
use crate::features::move_account::ports::{AccountRepository, OuRepository};
use crate::features::move_account::error::MoveAccountError;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use shared::domain::hrn::Hrn;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockAccountRepository {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl MockAccountRepository {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn create_test_account(&self, name: String, parent_hrn: Hrn) -> Account {
        Account::new(name, parent_hrn)
    }
    
    pub async fn save_initial(&self, account: Account) {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account);
    }
}

#[async_trait]
impl AccountRepository for MockAccountRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, MoveAccountError> {
        let accounts = self.accounts.lock().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }
    
    async fn save(&self, account: Account) -> Result<(), MoveAccountError> {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

pub struct MockOuRepository {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
}

impl MockOuRepository {
    pub fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn create_test_ou(&self, name: String, parent_hrn: Hrn) -> OrganizationalUnit {
        OrganizationalUnit::new(name, parent_hrn)
    }
    
    pub async fn save_initial(&self, ou: OrganizationalUnit) {
        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
    }
}

#[async_trait]
impl OuRepository for MockOuRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, MoveAccountError> {
        let ous = self.ous.lock().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }
    
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), MoveAccountError> {
        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}

// Implementaciones de clonación para los mocks
#[cfg(test)]
impl Clone for MockAccountRepository {
    fn clone(&self) -> Self {
        Self {
            accounts: Arc::clone(&self.accounts),
        }
    }
}

#[cfg(test)]
impl Clone for MockOuRepository {
    fn clone(&self) -> Self {
        Self {
            ous: Arc::clone(&self.ous),
        }
    }
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/move_account/ports.rs">
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::move_account::error::MoveAccountError;
use policies::domain::Hrn;
use async_trait::async_trait;

#[async_trait]
pub trait AccountRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, MoveAccountError>;
    async fn save(&self, account: Account) -> Result<(), MoveAccountError>;
}

#[async_trait]
pub trait OuRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, MoveAccountError>;
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), MoveAccountError>;
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/use_case_test.rs">
use crate::features::move_account::use_case::MoveAccountUseCase;
use crate::features::move_account::dto::MoveAccountCommand;
use crate::features::move_account::error::MoveAccountError;
use crate::features::move_account::mocks::{MockAccountRepository, MockOuRepository};
use shared::domain::hrn::Hrn;
use std::sync::Arc;

#[tokio::test]
async fn test_move_account_success() {
    // Setup
    let mock_account_repo = MockAccountRepository::new();
    let mock_ou_repo = MockOuRepository::new();
    
    // Arrange
    let account_hrn = Hrn::generate("account");
    let source_ou_hrn = Hrn::generate("ou");
    let target_ou_hrn = Hrn::generate("ou");
    
    // Preparar datos iniciales
    let mut account = mock_account_repo.create_test_account("TestAccount".to_string(), source_ou_hrn.clone()).await;
    let mut source_ou = mock_ou_repo.create_test_ou("SourceOU".to_string(), Hrn::generate("root")).await;
    let mut target_ou = mock_ou_repo.create_test_ou("TargetOU".to_string(), Hrn::generate("root")).await;
    
    // Añadir cuenta a la OU de origen
    source_ou.add_child_account(account_hrn.clone());
    account.parent_hrn = source_ou_hrn.clone();
    
    // Guardar datos iniciales en los mocks
    mock_account_repo.save_initial(account).await;
    mock_ou_repo.save_initial(source_ou).await;
    mock_ou_repo.save_initial(target_ou).await;
    
    let use_case = MoveAccountUseCase::new(Arc::new(mock_account_repo.clone()), Arc::new(mock_ou_repo.clone()));
    let command = MoveAccountCommand {
        account_hrn: account_hrn.clone(),
        source_ou_hrn: source_ou_hrn.clone(),
        target_ou_hrn: target_ou_hrn.clone(),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    
    // Verificar que la cuenta se ha movido
    let moved_account = mock_account_repo.find_by_hrn(&account_hrn).await.unwrap().unwrap();
    assert_eq!(moved_account.parent_hrn, target_ou_hrn);
    
    // Verificar que la OU de origen ya no contiene la cuenta
    let source_ou = mock_ou_repo.find_by_hrn(&source_ou_hrn).await.unwrap().unwrap();
    assert!(!source_ou.child_accounts.contains(&account_hrn));
    
    // Verificar que la OU de destino ahora contiene la cuenta
    let target_ou = mock_ou_repo.find_by_hrn(&target_ou_hrn).await.unwrap().unwrap();
    assert!(target_ou.child_accounts.contains(&account_hrn));
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/use_case.rs">
use crate::features::move_account::ports::{AccountRepository, OuRepository};
use crate::features::move_account::dto::MoveAccountCommand;
use crate::features::move_account::error::MoveAccountError;
use std::sync::Arc;

pub struct MoveAccountUseCase<AR: AccountRepository, OR: OuRepository> {
    account_repository: Arc<AR>,
    ou_repository: Arc<OR>,
}

impl<AR: AccountRepository, OR: OuRepository> MoveAccountUseCase<AR, OR> {
    pub fn new(account_repository: Arc<AR>, ou_repository: Arc<OR>) -> Self {
        Self {
            account_repository,
            ou_repository,
        }
    }
    
    pub async fn execute(&self, command: MoveAccountCommand) -> Result<(), MoveAccountError> {
        // 1. Cargar la Account a mover
        let mut account = self.account_repository.find_by_hrn(&command.account_hrn).await?
            .ok_or(MoveAccountError::AccountNotFound)?;
        
        // 2. Cargar la OU de origen
        let mut source_ou = self.ou_repository.find_by_hrn(&command.source_ou_hrn).await?
            .ok_or(MoveAccountError::SourceOuNotFound)?;
        
        // 3. Cargar la OU de destino
        let mut target_ou = self.ou_repository.find_by_hrn(&command.target_ou_hrn).await?
            .ok_or(MoveAccountError::TargetOuNotFound)?;
        
        // 4. Llamar a source_ou.remove_child_account(...)
        source_ou.remove_child_account(&account.hrn);
        
        // 5. Llamar a account.set_parent(...)
        account.parent_hrn = command.target_ou_hrn.clone();
        
        // 6. Llamar a target_ou.add_child_account(...)
        target_ou.add_child_account(account.hrn.clone());
        
        // 7. Guardar los tres agregados modificados (account, source_ou, target_ou)
        self.account_repository.save(account).await?;
        self.ou_repository.save(source_ou).await?;
        self.ou_repository.save(target_ou).await?;
        
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/mod.rs">
pub mod create_account;
pub mod create_ou;
pub mod move_account;
pub mod create_scp;
pub mod attach_scp;
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/account_repository.rs">
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use thiserror::Error;
use policies::domain::Hrn;

#[derive(Debug, Error)]
pub enum AccountRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Account not found")]
    AccountNotFound,
}

#[async_trait]
pub trait AccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/mod.rs">
pub mod account_repository;
pub mod ou_repository;
pub mod scp_repository;
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/ou_repository.rs">
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use thiserror::Error;
use policies::domain::Hrn;

#[derive(Debug, Error)]
pub enum OuRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Organizational Unit not found")]
    OuNotFound,
}

#[async_trait]
pub trait OuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/scp_repository.rs">
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use crate::shared::domain::scp::ServiceControlPolicy;

/// Error type for SCP repository operations
#[derive(Debug, thiserror::Error)]
pub enum ScpRepositoryError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Service Control Policy not found: {0}")]
    NotFound(String),
}

/// Repository trait for ServiceControlPolicy entities
#[async_trait]
pub trait ScpRepository: Send + Sync {
    /// Save an SCP
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError>;
    
    /// Find an SCP by HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/hierarchy_service.rs">
use crate::shared::domain::{Account, OrganizationalUnit};
use crate::shared::application::ports::{AccountRepository, AccountRepositoryError, OuRepository, OuRepositoryError};
use policies::domain::Hrn;
use std::sync::Arc;

/// Servicio para manejar la jerarquía organizacional
pub struct HierarchyService<AR: AccountRepository, OR: OuRepository> {
    account_repo: Arc<AR>,
    ou_repo: Arc<OR>,
}

impl<AR: AccountRepository, OR: OuRepository> HierarchyService<AR, OR> {
    /// Crea una nueva instancia del servicio
    pub fn new(account_repo: Arc<AR>, ou_repo: Arc<OR>) -> Self {
        Self { account_repo, ou_repo }
    }

    /// Obtiene la cadena completa de OUs desde una cuenta hasta la raíz
    pub async fn get_parent_chain(&self, account_hrn: &Hrn) -> Result<Vec<OrganizationalUnit>, HierarchyError> {
        let mut chain = Vec::new();
        let mut current_hrn = account_hrn.clone();
        
        // Comenzar desde la cuenta
        let account = self.account_repo.find_account_by_hrn(&current_hrn).await?
            .ok_or(HierarchyError::AccountNotFound(current_hrn.clone()))?;
        
        // Ascender por la jerarquía
        current_hrn = account.parent_hrn.clone();
        while let Some(ou) = self.ou_repo.find_ou_by_hrn(&current_hrn).await? {
            chain.push(ou.clone());
            current_hrn = ou.parent_hrn.clone();
        }
        
        Ok(chain)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HierarchyError {
    #[error("Account not found: {0}")]
    AccountNotFound(Hrn),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
}
</file>

<file path="crates/hodei-organizations/src/shared/application/mod.rs">
pub mod ports;
</file>

<file path="crates/hodei-organizations/src/shared/domain/account.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}

impl Account {
    pub fn new(hrn: Hrn, name: String, parent_hrn: Hrn) -> Self {
        let hrn = hrn;
        let name = name;

        Self { hrn, name, parent_hrn }
    }
    
    pub fn set_parent(&mut self, parent_hrn: Hrn) {
        self.parent_hrn = parent_hrn;
    }
    
    pub fn attach_scp(&mut self, _scp_hrn: Hrn) {
        // In a real implementation, this would add the SCP to a collection
        // For now, this is just a placeholder to match the OU interface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_new() {
        let hrn = Hrn::new(
            "default".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "test-account".to_string(),
        );
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-1".to_string(),
        );
        let account = Account::new("TestAccount".to_string(), parent_hrn.clone());
        
        assert_eq!(account.name, "TestAccount");
        assert_eq!(account.parent_hrn, parent_hrn);
        assert!(!account.hrn.to_string().is_empty());
    }
    
    #[test]
    fn test_account_set_parent() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-2".to_string(),
        );
        let new_parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-3".to_string(),
        );
        let mut account = Account::new("TestAccount".to_string(), parent_hrn.clone());
        
        assert_eq!(account.parent_hrn, parent_hrn);
        
        account.set_parent(new_parent_hrn.clone());
        assert_eq!(account.parent_hrn, new_parent_hrn);
    }
    
    #[test]
    fn test_account_attach_scp() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-4".to_string(),
        );
        let mut account = Account::new("TestAccount".to_string(), parent_hrn.clone());
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-1".to_string(),
        );

        // This should not panic
        account.attach_scp(scp_hrn);
    }
    
    #[test]
    fn test_account_clone() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-5".to_string(),
        );
        let account = Account::new("TestAccount".to_string(), parent_hrn.clone());
        let cloned_account = account.clone();
        
        assert_eq!(account.hrn, cloned_account.hrn);
        assert_eq!(account.name, cloned_account.name);
        assert_eq!(account.parent_hrn, cloned_account.parent_hrn);
    }
    
    #[test]
    fn test_account_debug() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-6".to_string(),
        );
        let account = Account::new("TestAccount".to_string(), parent_hrn.clone());
        let debug_str = format!("{:?}", account);
        
        assert!(debug_str.contains("Account"));
        assert!(debug_str.contains("TestAccount"));
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/mod.rs">
pub mod account;
pub mod ou;
pub mod scp;
</file>

<file path="crates/hodei-organizations/src/shared/domain/ou_test.rs">
use crate::shared::domain::OrganizationalUnit;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_ou_add_child_account() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let account_hrn = Hrn::new("account", "test-account");
    ou.add_child_account(account_hrn.clone());
    
    assert!(ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_account() {
    let account_hrn = Hrn::new("account", "test-account");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.add_child_account(account_hrn.clone());
    assert!(ou.child_accounts.contains(&account_hrn.to_string()));
    
    ou.remove_child_account(account_hrn.clone());
    assert!(!ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_add_child_ou() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let child_ou_hrn = Hrn::new("ou", "child-ou");
    ou.add_child_ou(child_ou_hrn.clone());
    
    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_ou() {
    let child_ou_hrn = Hrn::new("ou", "child-ou");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.add_child_ou(child_ou_hrn.clone());
    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));
    
    ou.remove_child_ou(child_ou_hrn.clone());
    assert!(!ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_attach_scp() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let scp_hrn = Hrn::new("scp", "test-scp");
    ou.attach_scp(scp_hrn.clone());
    
    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));
}

#[test]
fn test_ou_detach_scp() {
    let scp_hrn = Hrn::new("scp", "test-scp");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.attach_scp(scp_hrn.clone());
    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));
    
    ou.detach_scp(scp_hrn.clone());
    assert!(!ou.attached_scps.contains(&scp_hrn.to_string()));
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/ou.rs">
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnit {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
    pub child_ous: HashSet<Hrn>,
    pub child_accounts: HashSet<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl OrganizationalUnit {
    pub fn new(name: String, parent_hrn: Hrn) -> Self {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            name.clone(),
        );
        Self {
            hrn,
            name,
            parent_hrn,
            child_ous: HashSet::new(),
            child_accounts: HashSet::new(),
            attached_scps: HashSet::new(),
        }
    }
    
    pub fn add_child_ou(&mut self, child_hrn: Hrn) {
        self.child_ous.insert(child_hrn);
    }
    
    pub fn remove_child_ou(&mut self, child_hrn: &Hrn) {
        self.child_ous.remove(child_hrn);
    }
    
    pub fn add_child_account(&mut self, account_hrn: Hrn) {
        self.child_accounts.insert(account_hrn);
    }
    
    pub fn remove_child_account(&mut self, account_hrn: &Hrn) {
        self.child_accounts.remove(account_hrn);
    }
    
    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }
    
    pub fn detach_scp(&mut self, scp_hrn: &Hrn) {
        self.attached_scps.remove(scp_hrn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_ou_new() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-1".to_string(),
        );
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent_hrn.clone());
        
        assert_eq!(ou.name, "TestOU");
        assert_eq!(ou.parent_hrn, parent_hrn);
        assert!(ou.child_ous.is_empty());
        assert!(ou.child_accounts.is_empty());
        assert!(ou.attached_scps.is_empty());
        assert!(!ou.hrn.to_string().is_empty());
    }
    
    #[test]
    fn test_add_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-1".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 1);
    }
    
    #[test]
    fn test_remove_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-2".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        
        ou.remove_child_ou(&child_hrn);
        assert!(!ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 0);
    }
    
    #[test]
    fn test_add_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-1".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 1);
    }
    
    #[test]
    fn test_remove_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-2".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        
        ou.remove_child_account(&account_hrn);
        assert!(!ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 0);
    }
    
    #[test]
    fn test_attach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-1".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 1);
    }
    
    #[test]
    fn test_detach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-2".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        
        ou.detach_scp(&scp_hrn);
        assert!(!ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 0);
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/scp_test.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_scp_new() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    
    assert_eq!(scp.hrn, hrn);
    assert_eq!(scp.name, name);
    assert_eq!(scp.document, document);
}

#[test]
fn test_scp_clone() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let cloned_scp = scp.clone();
    
    assert_eq!(scp.hrn, cloned_scp.hrn);
    assert_eq!(scp.name, cloned_scp.name);
    assert_eq!(scp.document, cloned_scp.document);
}

#[test]
fn test_scp_debug() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let debug_str = format!("{:?}", scp);
    
    assert!(debug_str.contains("ServiceControlPolicy"));
    assert!(debug_str.contains("test-scp"));
    assert!(debug_str.contains("Test SCP"));
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/scp.rs">
use policies::shared::domain::hrn::Hrn;
use serde::{Deserialize, Serialize};

/// Represents a Service Control Policy in the organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceControlPolicy {
    /// Unique identifier for the SCP
    pub hrn: Hrn,
    /// Name of the SCP
    pub name: String,
    /// Policy document in Cedar format
    pub document: String,
}

impl ServiceControlPolicy {
    /// Create a new Service Control Policy
    pub fn new(hrn: Hrn, name: String, document: String) -> Self {
        Self {
            hrn,
            name,
            document,
        }
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/account_repository.rs">
use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::domain::account::Account;
use policies::domain::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use async_trait::async_trait;

pub struct SurrealAccountRepository {
    db: Surreal<Mem>,
}

impl SurrealAccountRepository {
    pub fn new(db: Surreal<Mem>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AccountRepository for SurrealAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let hrn_str = account.hrn.to_string();
        self.db.create(("account", &hrn_str)).content(account).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<Account> = self.db.select(("account", &hrn_str)).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs">
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::dto::GetEffectiveScpsCommand;
use crate::features::get_effective_scps::di::get_effective_scps_use_case;
use crate::shared::infrastructure::surreal::{SurrealScpRepository, SurrealAccountRepository, SurrealOuRepository};
use hodei_authorizer::ports::{OrganizationBoundaryProvider, AuthorizationError};
use hodei_authorizer::ports::ServiceControlPolicy;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;

/// SurrealDB implementation of OrganizationBoundaryProvider
pub struct SurrealOrganizationBoundaryProvider {
    db: Surreal<Any>,
}

impl SurrealOrganizationBoundaryProvider {
    /// Create a new SurrealOrganizationBoundaryProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    /// Get effective SCPs for an entity
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, AuthorizationError> {
        // Create repositories
        let scp_repository = SurrealScpRepository::new(self.db.clone());
        let account_repository = SurrealAccountRepository::new(self.db.clone());
        let ou_repository = SurrealOuRepository::new(self.db.clone());
        
        // Create use case
        let use_case = get_effective_scps_use_case(scp_repository, account_repository, ou_repository);
        
        // Create command
        let command = GetEffectiveScpsCommand {
            target_hrn: entity_hrn.to_string(),
        };
        
        // Execute use case
        let result = use_case.execute(command).await
            .map_err(|e| AuthorizationError::OrganizationBoundaryProvider(e.to_string()))?;
        
        // Convert to ServiceControlPolicy objects
        let mut scps = Vec::new();
        for scp_hrn_string in result.effective_scps {
            let scp_hrn = Hrn::from_str(&scp_hrn_string)
                .map_err(|e| AuthorizationError::OrganizationBoundaryProvider(e.to_string()))?;
            
            // Find the actual SCP object
            let scp_repository = SurrealScpRepository::new(self.db.clone());
            let scp = scp_repository.find_by_hrn(&scp_hrn).await
                .map_err(|e| AuthorizationError::OrganizationBoundaryProvider(e.to_string()))?
                .ok_or_else(|| AuthorizationError::OrganizationBoundaryProvider(format!("SCP not found: {}", scp_hrn_string)))?;
            
            scps.push(scp);
        }
        
        Ok(scps)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/ou_repository.rs">
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::shared::domain::ou::OrganizationalUnit;
use policies::domain::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use async_trait::async_trait;

pub struct SurrealOuRepository {
    db: Surreal<Mem>,
}

impl SurrealOuRepository {
    pub fn new(db: Surreal<Mem>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OuRepository for SurrealOuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        self.db.create(("ou", &hrn_str)).content(ou).await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<OrganizationalUnit> = self.db.select(("ou", &hrn_str)).await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/scp_repository.rs">
use crate::shared::domain::ServiceControlPolicy;
use crate::shared::application::ports::{ScpRepository, ScpRepositoryError};
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::sql::Thing;
use tracing::instrument;

/// SurrealDB implementation of ScpRepository
pub struct SurrealScpRepository {
    db: Surreal<Any>,
}

impl SurrealScpRepository {
    /// Create a new SurrealScpRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl ScpRepository for SurrealScpRepository {
    /// Save a service control policy
    #[instrument(skip(self, scp))]
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let hrn_string = scp.hrn.to_string();
        let thing = Thing::from(("scp", hrn_string.as_str()));
        
        self.db.update::<Option<ServiceControlPolicy>>(thing)
            .content(scp)
            .await
            .map_err(|e| ScpRepositoryError::SaveFailed(e.to_string()))?;
        
        Ok(())
    }

    /// Find a service control policy by HRN
    #[instrument(skip(self))]
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let hrn_string = hrn.to_string();
        let thing = Thing::from(("scp", hrn_string.as_str()));
        
        let result = self.db.select::<Option<ServiceControlPolicy>>(thing)
            .await
            .map_err(|e| ScpRepositoryError::NotFound(e.to_string()))?;
        
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/mod.rs">
pub mod domain;
pub mod application;
</file>

<file path="crates/hodei-organizations/src/lib.rs">
pub mod shared;
pub mod features;
</file>

<file path="crates/hodei-organizations/tests/attach_scp_test.rs">
use hodei_organizations::shared::domain::ou::OrganizationalUnit;
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;
use policies::domain::Hrn;
use hodei_organizations::features::attach_scp::use_case::AttachScpUseCase;
use hodei_organizations::features::attach_scp::dto::AttachScpCommand;
use hodei_organizations::features::create_ou::use_case::CreateOuUseCase;
use hodei_organizations::features::create_ou::dto::CreateOuCommand;
use hodei_organizations::features::create_scp::use_case::CreateScpUseCase;
use hodei_organizations::features::create_scp::dto::CreateScpCommand;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use hodei_organizations::shared::domain::account::Account;

// In-memory implementation of repositories for testing
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
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::OuRepositoryPort for InMemoryOuRepository {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, hodei_organizations::shared::application::ports::OuRepositoryError> {
        let ous = self.ous.lock().await;
        Ok(ous.get(&hrn.to_string()).cloned())
    }

    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), hodei_organizations::shared::application::ports::OuRepositoryError> {
        self.ous.lock().await.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}

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
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::ScpRepositoryPort for InMemoryScpRepository {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, hodei_organizations::shared::application::ports::ScpRepositoryError> {
        let scps = self.scps.lock().await;
        Ok(scps.get(&hrn.to_string()).cloned())
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
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::AccountRepositoryPort for InMemoryAccountRepository {
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, hodei_organizations::shared::application::ports::AccountRepositoryError> {
        let accounts = self.accounts.lock().await;
        Ok(accounts.get(&hrn.to_string()).cloned())
    }

    async fn save_account(&self, account: Account) -> Result<(), hodei_organizations::shared::application::ports::AccountRepositoryError> {
        self.accounts.lock().await.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

#[tokio::test]
async fn test_attach_scp_to_ou() {
    // Setup
    let ou_repo = InMemoryOuRepository::new();
    let scp_repo = InMemoryScpRepository::new();
    let account_repo = InMemoryAccountRepository::new();
    
    // Create OU
    let create_ou_use_case = CreateOuUseCase::new(Arc::new(ou_repo.clone()));
    let ou_command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "root".to_string(),
            "root-1".to_string(),
        ),
    };
    let ou_view = create_ou_use_case.execute(ou_command).await.unwrap();
    
    // Create SCP
    let create_scp_use_case = CreateScpUseCase::new(Arc::new(scp_repo.clone()));
    let scp_command = CreateScpCommand {
        name: "TestSCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };
    let scp_view = create_scp_use_case.execute(scp_command).await.unwrap();
    
    // Attach SCP to OU
    let attach_scp_use_case = AttachScpUseCase::new(scp_repo.clone(), account_repo.clone(), ou_repo.clone());
    let attach_command = AttachScpCommand {
        scp_hrn: scp_view.hrn.to_string(),
        target_hrn: ou_view.hrn.to_string(),
    };
    
    let result = attach_scp_use_case.execute(attach_command).await;
    assert!(result.is_ok());
    
    // Verify that the SCP was attached to the OU
    let ous = ou_repo.ous.lock().await;
    let ou = ous.get(&ou_view.hrn.to_string()).unwrap();
    assert!(ou.attached_scps.contains(&scp_view.hrn));
}
</file>

<file path="crates/hodei-organizations/tests/create_account_test.rs">
use hodei_organizations::features::create_account::use_case::CreateAccountUseCase;
use hodei_organizations::features::create_account::dto::CreateAccountCommand;
use hodei_organizations::shared::infrastructure::surreal::account_repository::SurrealAccountRepository;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_account_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear el repositorio
    let repository = SurrealAccountRepository::new(db);
    
    // Arrange: Instanciar el caso de uso
    let use_case = CreateAccountUseCase::new(Arc::new(repository));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "hodei".to_string(),
        "default".to_string(),
        "ou".to_string(),
        "parent-1".to_string(),
    );
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: parent_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que el AccountView devuelto es correcto
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, parent_hrn);
    assert!(!account_view.hrn.to_string().is_empty());
}
</file>

<file path="crates/hodei-organizations/tests/create_ou_test.rs">
use hodei_organizations::features::create_ou::use_case::CreateOuUseCase;
use hodei_organizations::features::create_ou::dto::CreateOuCommand;
use hodei_organizations::shared::infrastructure::surreal::ou_repository::SurrealOuRepository;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_ou_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear el repositorio
    let repository = SurrealOuRepository::new(db);
    
    // Arrange: Instanciar el caso de uso
    let use_case = CreateOuUseCase::new(Arc::new(repository));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "hodei".to_string(),
        "default".to_string(),
        "ou".to_string(),
        "parent-1".to_string(),
    );
    let command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que el OuView devuelto es correcto
    assert!(result.is_ok());
    let ou_view = result.unwrap();
    assert_eq!(ou_view.name, "TestOU");
    assert_eq!(ou_view.parent_hrn, parent_hrn);
    assert!(!ou_view.hrn.to_string().is_empty());
}
</file>

<file path="crates/hodei-organizations/tests/create_scp_test.rs">
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
</file>

<file path="crates/hodei-organizations/tests/get_effective_scps_test.rs">
use hodei_organizations::shared::domain::ou::OrganizationalUnit;
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;
use hodei_organizations::shared::domain::hrn::Hrn;
use hodei_organizations::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use hodei_organizations::features::create_ou::use_case::CreateOuUseCase;
use hodei_organizations::features::create_ou::dto::CreateOuCommand;
use hodei_organizations::features::create_scp::use_case::CreateScpUseCase;
use hodei_organizations::features::create_scp::dto::CreateScpCommand;
use hodei_organizations::features::attach_scp::use_case::AttachScpUseCase;
use hodei_organizations::features::attach_scp::dto::AttachScpCommand;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use hodei_organizations::shared::domain::account::Account;
use policies::domain::Hrn;

// In-memory implementation for testing
#[derive(Clone)]
struct InMemoryRepository {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
    scps: Arc<Mutex<HashMap<String, ServiceControlPolicy>>>,
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl InMemoryRepository {
    fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
            scps: Arc::new(Mutex::new(HashMap::new())),
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl hodei_organizations::features::get_effective_scps::ports::OuRepositoryPort for InMemoryRepository {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, hodei_organizations::shared::application::ports::OuRepositoryError> {
        let ous = self.ous.lock().await;
        Ok(ous.get(&hrn.to_string()).cloned())
    }
}

#[async_trait::async_trait]
impl hodei_organizations::features::get_effective_scps::ports::ScpRepositoryPort for InMemoryRepository {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, hodei_organizations::shared::application::ports::ScpRepositoryError> {
        let scps = self.scps.lock().await;
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::OuRepositoryPort for InMemoryRepository {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, hodei_organizations::shared::application::ports::OuRepositoryError> {
        let ous = self.ous.lock().await;
        Ok(ous.get(&hrn.to_string()).cloned())
    }

    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), hodei_organizations::shared::application::ports::OuRepositoryError> {
        self.ous.lock().await.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::ScpRepositoryPort for InMemoryRepository {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, hodei_organizations::shared::application::ports::ScpRepositoryError> {
        let scps = self.scps.lock().await;
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

#[async_trait::async_trait]
impl hodei_organizations::features::attach_scp::ports::AccountRepositoryPort for InMemoryRepository {
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, hodei_organizations::shared::application::ports::AccountRepositoryError> {
        let accounts = self.accounts.lock().await;
        Ok(accounts.get(&hrn.to_string()).cloned())
    }

    async fn save_account(&self, account: Account) -> Result<(), hodei_organizations::shared::application::ports::AccountRepositoryError> {
        self.accounts.lock().await.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

#[tokio::test]
async fn test_get_effective_scps_for_ou_with_attached_scp() {
    // Setup
    let repo = InMemoryRepository::new();
    
    // Create OU
    let create_ou_use_case = CreateOuUseCase::new(Arc::new(repo.clone()));
    let ou_command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: Hrn::generate("root"),
    };
    let ou_view = create_ou_use_case.execute(ou_command).await.unwrap();
    
    // Create SCP
    let create_scp_use_case = CreateScpUseCase::new(Arc::new(repo.clone()));
    let scp_command = CreateScpCommand {
        name: "TestSCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };
    let scp_view = create_scp_use_case.execute(scp_command).await.unwrap();
    
    // Attach SCP to OU
    let attach_scp_use_case = AttachScpUseCase::new(repo.clone(), repo.clone(), repo.clone());
    let attach_command = AttachScpCommand {
        scp_hrn: scp_view.hrn.to_string(),
        target_hrn: ou_view.hrn.to_string(),
    };
    attach_scp_use_case.execute(attach_command).await.unwrap();
    
    // Get effective SCPs
    let get_effective_scps_use_case = GetEffectiveScpsUseCase::new(repo.clone(), repo.clone());
    let effective_scps = get_effective_scps_use_case.execute(ou_view.hrn.to_string()).await.unwrap();
    
    // Verify that we get the attached SCP
    assert_eq!(effective_scps.len(), 1);
    assert_eq!(effective_scps[0].hrn, scp_view.hrn);
    assert_eq!(effective_scps[0].name, "TestSCP");
}

#[tokio::test]
async fn test_get_effective_scps_for_ou_without_scps() {
    // Setup
    let repo = InMemoryRepository::new();
    
    // Create OU
    let create_ou_use_case = CreateOuUseCase::new(Arc::new(repo.clone()));
    let ou_command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: Hrn::generate("root"),
    };
    let ou_view = create_ou_use_case.execute(ou_command).await.unwrap();
    
    // Get effective SCPs
    let get_effective_scps_use_case = GetEffectiveScpsUseCase::new(repo.clone(), repo.clone());
    let effective_scps = get_effective_scps_use_case.execute(ou_view.hrn.to_string()).await.unwrap();
    
    // Verify that we get no SCPs
    assert_eq!(effective_scps.len(), 0);
}

#[tokio::test]
async fn test_get_effective_scps_with_multiple_scps() {
    // Setup
    let repo = InMemoryRepository::new();
    
    // Create OU
    let create_ou_use_case = CreateOuUseCase::new(Arc::new(repo.clone()));
    let ou_command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: Hrn::generate("root"),
    };
    let ou_view = create_ou_use_case.execute(ou_command).await.unwrap();
    
    // Create multiple SCPs
    let create_scp_use_case = CreateScpUseCase::new(Arc::new(repo.clone()));
    
    let scp_command1 = CreateScpCommand {
        name: "TestSCP1".to_string(),
        document: "permit(principal, action::\"s3:GetObject\", resource);".to_string(),
    };
    let scp_view1 = create_scp_use_case.execute(scp_command1).await.unwrap();
    
    let scp_command2 = CreateScpCommand {
        name: "TestSCP2".to_string(),
        document: "forbid(principal, action::\"ec2:TerminateInstances\", resource);".to_string(),
    };
    let scp_view2 = create_scp_use_case.execute(scp_command2).await.unwrap();
    
    // Attach SCPs to OU
    let attach_scp_use_case = AttachScpUseCase::new(repo.clone(), repo.clone(), repo.clone());
    
    let attach_command1 = AttachScpCommand {
        scp_hrn: scp_view1.hrn.to_string(),
        target_hrn: ou_view.hrn.to_string(),
    };
    attach_scp_use_case.execute(attach_command1).await.unwrap();
    
    let attach_command2 = AttachScpCommand {
        scp_hrn: scp_view2.hrn.to_string(),
        target_hrn: ou_view.hrn.to_string(),
    };
    attach_scp_use_case.execute(attach_command2).await.unwrap();
    
    // Get effective SCPs
    let get_effective_scps_use_case = GetEffectiveScpsUseCase::new(repo.clone(), repo.clone());
    let effective_scps = get_effective_scps_use_case.execute(ou_view.hrn.to_string()).await.unwrap();
    
    // Verify that we get both SCPs
    assert_eq!(effective_scps.len(), 2);
    
    let scp_hrn_set: std::collections::HashSet<_> = effective_scps.iter().map(|scp| &scp.hrn).collect();
    assert!(scp_hrn_set.contains(&scp_view1.hrn));
    assert!(scp_hrn_set.contains(&scp_view2.hrn));
}
</file>

<file path="crates/hodei-organizations/tests/move_account_test.rs">
use hodei_organizations::features::move_account::use_case::MoveAccountUseCase;
use hodei_organizations::features::move_account::dto::MoveAccountCommand;
use hodei_organizations::shared::infrastructure::surreal::account_repository::SurrealAccountRepository;
use hodei_organizations::shared::infrastructure::surreal::ou_repository::SurrealOuRepository;
use hodei_organizations::shared::domain::ou::OrganizationalUnit;

use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use std::sync::Arc;
use hodei_organizations::shared::domain::account::Account;
use policies::domain::Hrn;

#[tokio::test]
async fn test_move_account_integration() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp", una OU "Staging" y una OU "Production"
    let staging_ou = OrganizationalUnit::new(
        "Staging".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-1".to_string()),
    );
    let production_ou = OrganizationalUnit::new(
        "Production".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-2".to_string()),
    );

    // Guardar las OUs
    ou_repository.save(&staging_ou).await.unwrap();
    ou_repository.save(&production_ou).await.unwrap();
    
    // Crear la cuenta WebApp inicialmente en Staging
    let webapp_account = Account::new("WebApp".to_string(), staging_ou.hrn.clone());
    
    // Guardar la cuenta
    account_repository.save(&webapp_account).await.unwrap();
    
    // Añadir la cuenta a la OU de Staging
    let mut staging_ou_with_account = staging_ou.clone();
    staging_ou_with_account.add_child_account(webapp_account.hrn.clone());
    ou_repository.save(&staging_ou_with_account).await.unwrap();
    
    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository.clone()), Arc::new(ou_repository.clone()));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: staging_ou_with_account.hrn.clone(),
        target_ou_hrn: production_ou.hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación fue exitosa
    assert!(result.is_ok());
    
    // Verificar que la cuenta se ha movido a Production
    let moved_account = account_repository.find_by_hrn(&webapp_account.hrn).await.unwrap().unwrap();
    assert_eq!(moved_account.parent_hrn, production_ou.hrn);
    
    // Verificar que la OU "Staging" ya no contiene la cuenta
    let updated_staging_ou = ou_repository.find_by_hrn(&staging_ou.hrn).await.unwrap().unwrap();
    assert!(!updated_staging_ou.child_accounts.contains(&webapp_account.hrn.to_string()));
    
    // Verificar que la OU "Production" ahora contiene la cuenta
    let updated_production_ou = ou_repository.find_by_hrn(&production_ou.hrn).await.unwrap().unwrap();
    assert!(updated_production_ou.child_accounts.contains(&webapp_account.hrn.to_string()));
}

#[tokio::test]
async fn test_move_account_source_not_found() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp" y una OU "Production"
    let production_ou = OrganizationalUnit::new(
        "Production".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-3".to_string()),
    );
    ou_repository.save(&production_ou).await.unwrap();
    
    let webapp_account = Account::new(
        "WebApp".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "ou-1".to_string()),
    );
    account_repository.save(&webapp_account).await.unwrap();
    
    // Create a non-existent source OU HRN
    let non_existent_ou_hrn = Hrn::new(
        "aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "non-existent".to_string(),
    );

    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository), Arc::new(ou_repository));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: non_existent_ou_hrn.clone(),
        target_ou_hrn: production_ou.hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación falló
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Source OU not found");
}

#[tokio::test]
async fn test_move_account_target_not_found() {
    // Setup: Crear una base de datos en memoria
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("hodei").use_db("organizations").await.unwrap();
    
    // Setup: Crear los repositorios
    let account_repository = SurrealAccountRepository::new(db.clone());
    let ou_repository = SurrealOuRepository::new(db.clone());
    
    // Arrange: Crear una cuenta "WebApp" y una OU "Staging"
    let staging_ou = OrganizationalUnit::new(
        "Staging".to_string(),
        Hrn::new("aws".to_string(), "hodei".to_string(), "default".to_string(), "root".to_string(), "root-4".to_string()),
    );
    ou_repository.save(&staging_ou).await.unwrap();
    
    let webapp_account = Account::new("WebApp".to_string(), staging_ou.hrn.clone());
    account_repository.save(&webapp_account).await.unwrap();
    
    // Añadir la cuenta a la OU de Staging
    let mut staging_ou_with_account = staging_ou.clone();
    staging_ou_with_account.add_child_account(webapp_account.hrn.clone());
    ou_repository.save(&staging_ou_with_account).await.unwrap();
    
    // Create a non-existent target OU HRN
    let non_existent_ou_hrn = Hrn::new(
        "aws".to_string(), "hodei".to_string(), "default".to_string(), "ou".to_string(), "non-existent".to_string(),
    );

    // Crear el caso de uso
    let use_case = MoveAccountUseCase::new(Arc::new(account_repository), Arc::new(ou_repository));
    
    // Crear el comando para mover la cuenta
    let command = MoveAccountCommand {
        account_hrn: webapp_account.hrn.clone(),
        source_ou_hrn: staging_ou_with_account.hrn.clone(),
        target_ou_hrn: non_existent_ou_hrn.clone(),
    };
    
    // Act: Ejecutar el caso de uso
    let result = use_case.execute(command).await;
    
    // Assert: Verificar que la operación falló
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(format!("{}", error), "Target OU not found");
}
</file>

<file path="crates/hodei-organizations/Cargo.toml">
[package]
name = "hodei-organizations"
version = "0.1.0"
edition = "2024"

[dependencies]
shared = { path = "../shared" }
policies = { path = "../policies" }
surrealdb = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
</file>

<file path="crates/hodei-organizations/README.md">
# hodei-organizations

Este crate implementa la funcionalidad de gestión de organizaciones, cuentas y políticas de control de servicio (SCPs) siguiendo una arquitectura hexagonal y VSA.

## Estructura

```
src/
├── shared/
│   ├── domain/
│   │   ├── account.rs
│   │   ├── ou.rs
│   │   └── scp.rs
│   ├── application/
│   │   └── ports/
│   │       ├── account_repository.rs
│   │       ├── ou_repository.rs
│   │       └── scp_repository.rs
│   └── infrastructure/
│       └── surreal/
│           ├── account_repository.rs
│           ├── ou_repository.rs
│           └── scp_repository.rs
└── features/
    ├── create_account/
    │   ├── mod.rs
    │   ├── use_case.rs
    │   ├── ports.rs
    │   ├── error.rs
    │   ├── dto.rs
    │   ├── adapter.rs
    │   ├── use_case_test.rs
    │   └── mocks.rs
    ├── create_ou/
    ├── move_account/
    ├── create_scp/
    └── attach_scp/
```

## Features Implementadas

### create_account
Permite crear una nueva cuenta en la organización.

## Próximas Features

- create_ou
- move_account
- create_scp
- attach_scp
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/mod.rs">
/// Feature: Add User to Group
///
/// This feature allows adding users to existing groups

pub mod dto;
pub mod use_case;
pub mod di;

pub use use_case::AddUserToGroupUseCase;
</file>

<file path="crates/hodei-iam/src/features/create_group/mod.rs">
/// Feature: Create Group
///
/// This feature allows creating new groups in the IAM system

pub mod dto;
pub mod use_case;
pub mod di;

pub use dto::{CreateGroupCommand, GroupView};
pub use use_case::CreateGroupUseCase;
</file>

<file path="crates/hodei-iam/src/features/create_user/mod.rs">
/// Feature: Create User
///
/// This feature allows creating new users in the IAM system

pub mod dto;
pub mod use_case;
pub mod di;

pub use use_case::CreateUserUseCase;
</file>

<file path="crates/hodei-iam/src/features/mod.rs">
/// Features module for hodei-iam
///
/// This module contains all the use cases (features) organized as vertical slices

pub mod create_group;
pub mod create_user;
pub mod add_user_to_group;
</file>

<file path="crates/hodei-iam/src/shared/application/mod.rs">
/// Application layer for hodei-iam

pub mod ports;
mod di_configurator;

pub use di_configurator::configure_default_iam_entities;
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/mod.rs">
/// Infrastructure layer for hodei-iam
///
/// This module contains the adapters and implementations for infrastructure concerns
/// like persistence, external services, etc.

pub mod persistence;
</file>

<file path="crates/hodei-iam/src/shared/mod.rs">
/// Shared kernel for hodei-iam

pub mod domain;
pub mod application;
pub mod infrastructure;
</file>

<file path="crates/hodei-iam/Cargo.toml">
[package]
name = "hodei-iam"
version = "0.1.0"
edition = "2024"

[dependencies]
policies = { path = "../policies" }
cedar-policy = { workspace = true }
serde = { workspace = true }
anyhow = "1.0"
async-trait = "0.1"
tokio = { workspace = true }
uuid = { version = "1.0", features = ["v4", "serde"] }

[dev-dependencies]
tokio = { workspace = true, features = ["full", "test-util"] }
</file>

<file path="crates/policies/src/features/batch_eval/di.rs">
use anyhow::Result;

use super::use_case::BatchEvalUseCase;

pub async fn make_use_case_mem() -> Result<BatchEvalUseCase> {
    Ok(BatchEvalUseCase::new())
}
</file>

<file path="crates/policies/src/features/batch_eval/dto.rs">
use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto::{AuthorizationScenario, EntityDefinition, EvaluationStatistics};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchPlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub scenarios: Vec<AuthorizationScenario>,
    #[serde(default)]
    pub limit_scenarios: Option<usize>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPlaygroundResponse {
    pub results_count: usize,
    pub statistics: EvaluationStatistics,
}
</file>

<file path="crates/policies/src/features/batch_eval/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/batch_eval/use_case.rs">
use super::dto::{BatchPlaygroundRequest, BatchPlaygroundResponse};
use cedar_policy::{Entities, PolicySet};

use crate::features::policy_playground::dto as base;
use crate::shared::application::parallel::{
    build_entities as build_entities_shared,
    build_policy_set as build_policy_set_shared,
    evaluate_scenarios_channel,
    AuthScenario,
};
use tracing::info;

#[derive(Default)]
pub struct BatchEvalUseCase;

impl BatchEvalUseCase {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, req: &BatchPlaygroundRequest) -> Result<BatchPlaygroundResponse, String> {
        // Apply limit
        let scenarios = if let Some(limit) = req.limit_scenarios {
            req.scenarios.iter().cloned().take(limit).collect::<Vec<_>>()
        } else { req.scenarios.clone() };

        // Build shared PolicySet and Entities
        let pset = build_policy_set_shared(&req.policies).unwrap_or_else(|_| PolicySet::new());
        let entity_tuples: Vec<(String, std::collections::HashMap<String, serde_json::Value>, Vec<String>)> = req
            .entities
            .iter()
            .map(|e| (e.uid.clone(), e.attributes.clone(), e.parents.clone()))
            .collect();
        let ents = build_entities_shared(&entity_tuples).unwrap_or_else(|_| Entities::empty());

        // Build scenarios for the evaluator
        let total = scenarios.len();
        let auth_scenarios: Vec<AuthScenario> = scenarios
            .into_iter()
            .map(|s| AuthScenario {
                name: s.name,
                principal: s.principal,
                action: s.action,
                resource: s.resource,
                context: s.context,
            })
            .collect();

        // Use mpsc-based evaluator
        let workers = 8usize;
        let buffer = 2 * workers;
        let (outcomes, pstats) = evaluate_scenarios_channel(&pset, &ents, auth_scenarios, req.timeout_ms, workers, buffer).await?;

        let mut total_time = 0u64;
        let mut allow_count = 0usize;
        for o in outcomes.iter() {
            total_time += o.eval_time_us;
            if o.allow { allow_count += 1; }
        }

        let total = total;
        let statistics = base::EvaluationStatistics {
            total_scenarios: total,
            allow_count,
            deny_count: total.saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if total == 0 { 0 } else { total_time / total as u64 },
        };

        info!(
            scenarios_total = total,
            timeouts = pstats.timeouts,
            total_eval_time_us = pstats.total_eval_time_us,
            "batch_eval completed"
        );

        Ok(BatchPlaygroundResponse { results_count: total, statistics })
    }
}
</file>

<file path="crates/policies/src/features/policy_analysis/di.rs">
use anyhow::Result;

use super::use_case::AnalyzePoliciesUseCase;

pub async fn make_use_case_mem() -> Result<AnalyzePoliciesUseCase> {
    Ok(AnalyzePoliciesUseCase::new())
}
</file>

<file path="crates/policies/src/features/policy_analysis/dto.rs">
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzePoliciesRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub rules: Vec<AnalysisRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalysisRule {
    pub id: String,
    /// Example: "no_permit_without_mfa"
    pub kind: String,
    /// Optional data for rule (e.g., action name, resource type)
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzePoliciesResponse {
    pub passed: bool,
    #[serde(default)]
    pub violations: Vec<RuleViolation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleViolation {
    pub rule_id: String,
    pub message: String,
}
</file>

<file path="crates/policies/src/features/policy_analysis/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/policy_playground/dto.rs">
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub authorization_requests: Vec<AuthorizationScenario>,
    pub options: Option<PlaygroundOptions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityDefinition {
    pub uid: String,
    pub attributes: HashMap<String, serde_json::Value>,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthorizationScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundOptions {
    pub include_diagnostics: bool,
}

impl Default for PlaygroundOptions {
    fn default() -> Self {
        Self { include_diagnostics: true }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaygroundResponse {
    pub policy_validation: PolicyValidationResult,
    pub schema_validation: SchemaValidationResult,
    pub authorization_results: Vec<AuthorizationResult>,
    pub statistics: EvaluationStatistics,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub policies_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub entity_types_count: usize,
    pub actions_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationResult {
    pub scenario_name: String,
    pub decision: Decision,
    pub determining_policies: Vec<String>,
    pub evaluated_policies: Vec<PolicyEvaluation>,
    pub diagnostics: AuthorizationDiagnostics,
    pub evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEvaluation {
    pub policy_id: String,
    pub result: PolicyResult,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum PolicyResult {
    Permit,
    Forbid,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationDiagnostics {
    pub reasons: Vec<String>,
    pub errors: Vec<String>,
    pub info: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationStatistics {
    pub total_scenarios: usize,
    pub allow_count: usize,
    pub deny_count: usize,
    pub total_evaluation_time_us: u64,
    pub average_evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub policy_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: WarningSeverity,
}

#[derive(Debug, Clone, Serialize)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub enum Decision { Allow, Deny }
</file>

<file path="crates/policies/src/features/policy_playground/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/policy_playground_traces/di.rs">
use anyhow::Result;

use super::use_case::TracedPlaygroundUseCase;

pub async fn make_use_case_mem() -> Result<TracedPlaygroundUseCase> {
    Ok(TracedPlaygroundUseCase::new())
}
</file>

<file path="crates/policies/src/features/policy_playground_traces/dto.rs">
use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto as base;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TracedPlaygroundOptions {
    #[serde(default)]
    pub include_policy_traces: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedAuthorizationResult {
    pub base: base::AuthorizationResult,
    pub determining_policies: Option<Vec<String>>, // None when not requested or unavailable
    pub evaluated_policies: Option<Vec<base::PolicyEvaluation>>, // idem
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedPlaygroundResponse {
    pub policy_validation: base::PolicyValidationResult,
    pub schema_validation: base::SchemaValidationResult,
    pub authorization_results: Vec<TracedAuthorizationResult>,
    pub statistics: base::EvaluationStatistics,
}
</file>

<file path="crates/policies/src/features/policy_playground_traces/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/validate_policy/dto.rs">
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ValidatePolicyQuery {
    pub policy_content: String,
}

impl ValidatePolicyQuery {
    pub fn new(policy_content: String) -> Self {
        Self { policy_content }
    }

    pub fn validate(&self) -> Result<(), ValidatePolicyValidationError> {
        if self.policy_content.trim().is_empty() {
            return Err(ValidatePolicyValidationError::EmptyContent);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyValidationError {
    #[error("policy content cannot be empty")]
    EmptyContent,
}
</file>

<file path="crates/policies/src/features/validate_policy/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/shared/domain/entity_utils.rs">
//! Utility functions and helpers for working with HodeiEntity implementations.
//!
//! This module provides helper functions to make it easier to implement the HodeiEntity
//! trait, particularly for converting common Rust types to RestrictedExpression values
//! that can be used as entity attributes in Cedar policies.

use cedar_policy::RestrictedExpression;
use std::collections::{BTreeMap, HashMap};

/// Helper trait for converting common Rust types to RestrictedExpression
///
/// This trait provides convenient methods for converting standard Rust types
/// to RestrictedExpression values that can be used as entity attributes.
pub trait ToRestrictedExpression {
    /// Convert the value to a RestrictedExpression
    fn to_restricted_expr(&self) -> RestrictedExpression;
}

impl ToRestrictedExpression for String {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.clone())
    }
}

impl ToRestrictedExpression for &str {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.to_string())
    }
}

impl ToRestrictedExpression for bool {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_bool(*self)
    }
}

impl ToRestrictedExpression for i64 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self)
    }
}

impl ToRestrictedExpression for i32 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self as i64)
    }
}

impl<T: ToRestrictedExpression> ToRestrictedExpression for Vec<T> {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let expressions: Vec<RestrictedExpression> =
            self.iter().map(|item| item.to_restricted_expr()).collect();
        RestrictedExpression::new_set(expressions)
    }
}

impl<K, V> ToRestrictedExpression for HashMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

impl<K, V> ToRestrictedExpression for BTreeMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

/// Builder for creating entity attributes map
///
/// This provides a fluent API for building the attributes map required by HodeiEntity.
///
/// # Example
///
/// ```
/// use policies::domain::entity_utils::AttributesBuilder;
///
/// let attributes = AttributesBuilder::new()
///     .attr("name", "Alice")
///     .attr("age", 30i64)
///     .attr("active", true)
///     .attr("tags", vec!["employee", "fulltime"])
///     .build();
/// ```
pub struct AttributesBuilder {
    attributes: HashMap<String, RestrictedExpression>,
}

impl AttributesBuilder {
    /// Create a new AttributesBuilder
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the builder
    ///
    /// # Example
    ///
    /// ```
    /// use policies::domain::entity_utils::AttributesBuilder;
    ///
    /// let attributes = AttributesBuilder::new()
    ///     .attr("name", "Alice")
    ///     .attr("age", 30i64)
    ///     .build();
    /// ```
    pub fn attr<T: ToRestrictedExpression>(mut self, name: &str, value: T) -> Self {
        self.attributes
            .insert(name.to_string(), value.to_restricted_expr());
        self
    }

    /// Build the attributes map
    pub fn build(self) -> HashMap<String, RestrictedExpression> {
        self.attributes
    }
}

impl Default for AttributesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_conversions() {
        let string_expr = "test".to_restricted_expr();
        // RestrictedExpression implements Debug, not Display
        assert!(format!("{:?}", string_expr).contains("test"));

        let bool_expr = true.to_restricted_expr();
        assert!(format!("{:?}", bool_expr).to_lowercase().contains("true"));

        let int_expr = 42i64.to_restricted_expr();
        assert!(format!("{:?}", int_expr).contains("42"));
    }

    #[test]
    fn test_collection_conversions() {
        let vec_expr = vec!["a", "b", "c"].to_restricted_expr();
        let vec_str = format!("{:?}", vec_expr);
        assert!(!vec_str.is_empty());

        let mut map = HashMap::new();
        map.insert("key1", "value1");
        map.insert("key2", "value2");
        let map_expr = map.to_restricted_expr();
        let map_str = format!("{:?}", map_expr);
        assert!(!map_str.is_empty());
    }

    #[test]
    fn test_attributes_builder() {
        let attributes = AttributesBuilder::new()
            .attr("name", "Alice")
            .attr("age", 30i64)
            .attr("active", true)
            .attr("tags", vec!["employee", "fulltime"])
            .build();

        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("age"));
        assert!(attributes.contains_key("active"));
        assert!(attributes.contains_key("tags"));
    }
}
</file>

<file path="crates/policies/src/shared/domain/error.rs">
use crate::shared::domain::ports::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HodeiPoliciesError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Policy with ID '{0}' was not found")]
    NotFound(String),

    #[error("Storage error")]
    Storage(#[from] StorageError), // Automatic conversion from StorageError

    #[error("Error parsing policy: {0}")]
    PolicyParse(String),

    #[error("Policy is invalid according to schema: {0}")]
    PolicyValidation(String),

    #[error("Internal engine error: {0}")]
    Engine(String),
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/mod.rs">
pub mod mem_storage;

pub use mem_storage::SurrealMemStorage;

#[cfg(feature = "embedded")]
pub mod embedded_storage;

#[cfg(feature = "embedded")]
pub use embedded_storage::SurrealEmbeddedStorage;
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/di.rs">
use super::use_case::AddUserToGroupUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
/// Dependency Injection for add_user_to_group feature

use std::sync::Arc;

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo)
}
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/dto.rs">
/// Data Transfer Objects for add_user_to_group feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserToGroupCommand {
    pub user_hrn: String,
    pub group_hrn: String,
}
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/use_case.rs">
use super::dto::AddUserToGroupCommand;
use crate::shared::application::ports::{GroupRepository, UserRepository};
use policies::shared::domain::hrn::Hrn;
/// Use case for adding a user to a group

use std::sync::Arc;

pub struct AddUserToGroupUseCase {
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
}

impl AddUserToGroupUseCase {
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        group_repo: Arc<dyn GroupRepository>,
    ) -> Self {
        Self { user_repo, group_repo }
    }

    pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), anyhow::Error> {
        // Parse HRNs
        let user_hrn = Hrn::from_string(&cmd.user_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid user HRN: {}", cmd.user_hrn))?;
        let group_hrn = Hrn::from_string(&cmd.group_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid group HRN: {}", cmd.group_hrn))?;

        // Validate that the group exists to maintain consistency
        if self.group_repo.find_by_hrn(&group_hrn).await?.is_none() {
            return Err(anyhow::anyhow!("Group not found: {}", cmd.group_hrn));
        }

        // Load the user
        let mut user = self.user_repo.find_by_hrn(&user_hrn).await?
            .ok_or_else(|| anyhow::anyhow!("User not found: {}", cmd.user_hrn))?;

        // Add user to group (domain logic handles idempotency)
        user.add_to_group(group_hrn);

        // Persist the updated user
        self.user_repo.save(&user).await?;

        Ok(())
    }
}
</file>

<file path="crates/hodei-iam/src/features/create_group/di.rs">
use super::use_case::CreateGroupUseCase;
use crate::shared::application::ports::GroupRepository;
/// Dependency Injection for create_group feature

use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn GroupRepository>) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo)
}
</file>

<file path="crates/hodei-iam/src/features/create_group/dto.rs">
/// Data Transfer Objects for create_group feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupCommand {
    pub group_name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}
</file>

<file path="crates/hodei-iam/src/features/create_group/use_case.rs">
use super::dto::{CreateGroupCommand, GroupView};
use crate::shared::{
    application::ports::GroupRepository,
    domain::Group,
};
use policies::shared::domain::hrn::Hrn;
/// Use case for creating a new group

use std::sync::Arc;

pub struct CreateGroupUseCase {
    repo: Arc<dyn GroupRepository>,
}

impl CreateGroupUseCase {
    pub fn new(repo: Arc<dyn GroupRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let group_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            group_id,
        );

        // Create the group domain entity
        let mut group = Group::new(hrn, cmd.group_name.clone());
        group.tags = cmd.tags.clone();

        // Persist the group
        self.repo.save(&group).await?;

        // Return the view
        Ok(GroupView {
            hrn: group.hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}
</file>

<file path="crates/hodei-iam/src/features/create_user/di.rs">
use super::use_case::CreateUserUseCase;
use crate::shared::application::ports::UserRepository;
/// Dependency Injection for create_user feature

use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn UserRepository>) -> CreateUserUseCase {
    CreateUserUseCase::new(repo)
}
</file>

<file path="crates/hodei-iam/src/features/create_user/dto.rs">
/// Data Transfer Objects for create_user feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserCommand {
    pub name: String,
    pub email: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}
</file>

<file path="crates/hodei-iam/src/features/create_user/use_case.rs">
use super::dto::{CreateUserCommand, UserView};
use crate::shared::{
    application::ports::UserRepository,
    domain::User,
};
use policies::shared::domain::hrn::Hrn;
/// Use case for creating a new user

use std::sync::Arc;

pub struct CreateUserUseCase {
    repo: Arc<dyn UserRepository>,
}

impl CreateUserUseCase {
    pub fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let user_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            user_id,
        );

        // Create the user domain entity
        let mut user = User::new(hrn, cmd.name.clone(), cmd.email.clone());
        user.tags = cmd.tags.clone();

        // Persist the user
        self.repo.save(&user).await?;

        // Return the view
        Ok(UserView {
            hrn: user.hrn.to_string(),
            name: user.name,
            email: user.email,
            groups: Vec::new(),
            tags: user.tags,
        })
    }
}
</file>

<file path="crates/hodei-iam/src/shared/application/ports/mod.rs">
use crate::shared::domain::{Group, User};
/// Application ports (interfaces) for hodei-iam
///
/// This module defines the traits (ports) that the application layer uses
/// to interact with infrastructure concerns like persistence.

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error>;
}

#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error>;
}
</file>

<file path="crates/hodei-iam/src/shared/application/di_configurator.rs">
use crate::shared::domain::{CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User};
/// DI Configurator for hodei-iam
/// 
/// Provides a function to configure the policies EngineBuilder with default IAM entities

use anyhow::Result;
use policies::shared::application::EngineBuilder;

/// Configure an EngineBuilder with default IAM entities
/// 
/// This function registers:
/// - Principals: User, ServiceAccount
/// - Resources: User, Group, ServiceAccount, Namespace
/// - Actions: CreateUserAction, CreateGroupAction
/// 
/// # Example
/// ```no_run
/// use policies::shared::application::di_helpers;
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
/// # Ok(())
/// # }
/// ```
pub fn configure_default_iam_entities(mut builder: EngineBuilder) -> Result<EngineBuilder> {
    builder
        .register_principal::<User>()?
        .register_principal::<ServiceAccount>()?
        .register_resource::<User>()?
        .register_resource::<Group>()?
        .register_resource::<ServiceAccount>()?
        .register_resource::<Namespace>()?
        .register_action::<CreateUserAction>()?
        .register_action::<CreateGroupAction>()?;
    Ok(builder)
}
</file>

<file path="crates/hodei-iam/src/shared/domain/actions.rs">
use cedar_policy::EntityTypeName;
/// Domain actions for hodei-iam
/// 
/// This module defines the IAM actions that can be performed

use policies::shared::domain::ports::Action;
use std::str::FromStr;

pub struct CreateUserAction;

impl Action for CreateUserAction {
    fn name() -> &'static str {
        "create_user"
    }
    
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("User").expect("Valid entity type"),
        )
    }
}

pub struct CreateGroupAction;

impl Action for CreateGroupAction {
    fn name() -> &'static str {
        "create_group"
    }
    
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("Group").expect("Valid entity type"),
        )
    }
}
</file>

<file path="crates/hodei-iam/src/shared/domain/entities.rs">
use cedar_policy::{EntityUid, RestrictedExpression};
/// Domain entities for hodei-iam
/// 
/// This module defines the core IAM entities: User, Group, ServiceAccount, Namespace

use policies::shared::domain::hrn::Hrn;
use policies::shared::domain::ports::{self, HodeiEntity, HodeiEntityType, Principal, Resource};
use ports::AttributeType::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub hrn: Hrn,
    pub name: String,
    pub group_hrns: Vec<Hrn>,
    pub email: String,
    pub tags: Vec<String>,
}

impl User {
    /// Create a new User
    pub fn new(hrn: Hrn, name: String, email: String) -> Self {
        Self {
            hrn,
            name,
            email,
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Add user to a group (idempotent - won't add duplicates)
    pub fn add_to_group(&mut self, group_hrn: Hrn) {
        if !self.group_hrns.contains(&group_hrn) {
            self.group_hrns.push(group_hrn);
        }
    }

    /// Remove user from a group
    pub fn remove_from_group(&mut self, group_hrn: &Hrn) {
        self.group_hrns.retain(|hrn| hrn != group_hrn);
    }

    /// Get all groups this user belongs to
    pub fn groups(&self) -> &[Hrn] {
        &self.group_hrns
    }

    /// Get user's email
    pub fn email(&self) -> &str {
        &self.email
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub attached_policy_hrns: Vec<Hrn>,
}

impl Group {
    /// Create a new Group
    pub fn new(hrn: Hrn, name: String) -> Self {
        Self {
            hrn,
            name,
            tags: Vec::new(),
            attached_policy_hrns: Vec::new(),
        }
    }

    /// Attach a policy to this group (idempotent)
    pub fn attach_policy(&mut self, policy_hrn: Hrn) {
        if !self.attached_policy_hrns.contains(&policy_hrn) {
            self.attached_policy_hrns.push(policy_hrn);
        }
    }

    /// Detach a policy from this group
    pub fn detach_policy(&mut self, policy_hrn: &Hrn) {
        self.attached_policy_hrns.retain(|hrn| hrn != policy_hrn);
    }

    /// Get group name
    pub fn group_name(&self) -> &str {
        &self.name
    }

    /// Get attached policies
    pub fn attached_policies(&self) -> &[Hrn] {
        &self.attached_policy_hrns
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub hrn: Hrn,
    pub name: String,
    pub annotations: HashMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub annotations: HashMap<String, String>,
}

// --- Implementaciones para User ---

impl HodeiEntityType for User {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "User"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("email", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn parents(&self) -> Vec<EntityUid> {
        self.group_hrns.iter().map(|hrn| hrn.euid()).collect()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
}

impl Principal for User {}
impl Resource for User {}

// --- Implementaciones para Group ---

impl HodeiEntityType for Group {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "Group"
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Group {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Group {}

// --- Implementaciones para ServiceAccount ---

impl HodeiEntityType for ServiceAccount {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "ServiceAccount"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for ServiceAccount {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for ServiceAccount {}
impl Resource for ServiceAccount {}

// --- Implementaciones para Namespace ---

impl HodeiEntityType for Namespace {
    fn service_name() -> &'static str { "iam" }
    fn resource_type_name() -> &'static str { "Namespace" }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Namespace {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Namespace {}
</file>

<file path="crates/hodei-iam/src/shared/domain/mod.rs">
/// Domain layer for hodei-iam

pub mod entities;
pub mod actions;

pub use actions::{CreateGroupAction, CreateUserAction};
// Re-export for convenience
pub use entities::{Group, Namespace, ServiceAccount, User};
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/persistence/mod.rs">
use crate::shared::application::ports::{GroupRepository, UserRepository};
use crate::shared::domain::{Group, User};
/// In-memory repository implementations for testing
///
/// These repositories store data in memory using RwLock for thread-safe access

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::sync::RwLock;

/// In-memory implementation of UserRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: RwLock<Vec<User>>,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error> {
        let mut users = self.users.write().unwrap();

        // Remove existing user with same HRN if present
        users.retain(|u| u.hrn != user.hrn);

        // Add the new/updated user
        users.push(user.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error> {
        let users = self.users.read().unwrap();
        Ok(users.iter().find(|u| &u.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error> {
        let users = self.users.read().unwrap();
        Ok(users.clone())
    }
}

/// In-memory implementation of GroupRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryGroupRepository {
    groups: RwLock<Vec<Group>>,
}

impl InMemoryGroupRepository {
    pub fn new() -> Self {
        Self {
            groups: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GroupRepository for InMemoryGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error> {
        let mut groups = self.groups.write().unwrap();

        // Remove existing group with same HRN if present
        groups.retain(|g| g.hrn != group.hrn);

        // Add the new/updated group
        groups.push(group.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error> {
        let groups = self.groups.read().unwrap();
        Ok(groups.iter().find(|g| &g.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error> {
        let groups = self.groups.read().unwrap();
        Ok(groups.clone())
    }
}
</file>

<file path="crates/hodei-iam/src/lib.rs">
/// hodei-iam: Default IAM entities for the policies engine
/// 
/// This crate provides a standard set of Identity and Access Management entities
/// that can be used with the policies engine. It follows the same Vertical Slice
/// Architecture (VSA) with hexagonal architecture as the policies crate.
/// 
/// # Structure
/// - `shared/domain`: Core IAM entities (User, Group, ServiceAccount, Namespace) and actions
/// - `shared/application`: Ports (repository traits) and DI configurator
/// - `shared/infrastructure`: Infrastructure adapters (in-memory repositories for testing)
/// - `features`: IAM-specific features/use cases (create_user, create_group, add_user_to_group)
///
/// # Example
/// ```no_run
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// use policies::shared::application::di_helpers;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// // Build an engine with default IAM entities
/// let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
/// # Ok(())
/// # }
/// ```

pub mod shared;
pub mod features;

pub use shared::application::configure_default_iam_entities;
// Re-export commonly used items for convenience
pub use shared::domain::{CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User};

// Re-export features for easy access
pub use features::{
    add_user_to_group::AddUserToGroupUseCase,
    create_group::CreateGroupUseCase,
    create_user::CreateUserUseCase,
};

#[cfg(test)]
mod tests {
    use super::*;
    use policies::shared::domain::hrn::Hrn;
    use policies::shared::domain::ports::{Action, Principal, Resource};

    fn sample_group(id: &str) -> Group {
        Group {
            hrn: Hrn::new(
                "aws".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                id.into(),
            ),
            name: format!("group-{}", id),
            tags: vec!["team".into()],
            attached_policy_hrns: vec![],
        }
    }

    #[test]
    fn group_attributes_contains_expected_keys() {
        use policies::shared::domain::ports::HodeiEntity;
        let g = sample_group("dev");
        let attrs = g.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("tags"));
    }

    #[test]
    fn user_parents_produce_entityuids() {
        let groups = vec![
            Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                "dev".into(),
            ),
            Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                "ops".into(),
            ),
        ];
        let user = User {
            hrn: Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "User".into(),
                "alice".into(),
            ),
            name: "Alice".into(),
            group_hrns: groups,
            email: "alice@example.com".into(),
            tags: vec!["admin".into()],
        };
        use policies::shared::domain::ports::HodeiEntity;
        let parents = user.parents();
        assert_eq!(parents.len(), 2);
        let s0 = format!("{}", parents[0]);
        assert!(s0.contains("Group"));
    }

    #[test]
    fn user_attributes_contains_expected() {
        let user = User {
            hrn: Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "User".into(),
                "alice".into(),
            ),
            name: "Alice".into(),
            group_hrns: vec![],
            email: "alice@example.com".into(),
            tags: vec!["owner".into()],
        };
        use policies::shared::domain::ports::HodeiEntity;
        let attrs = user.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("email"));
        assert!(attrs.contains_key("tags"));
    }
    
    #[test]
    fn user_implements_principal_trait() {
        fn assert_is_principal<T: Principal>() {}
        assert_is_principal::<User>();
    }
    
    #[test]
    fn user_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<User>();
    }
    
    #[test]
    fn group_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<Group>();
    }
    
    #[test]
    fn service_account_implements_principal_trait() {
        fn assert_is_principal<T: Principal>() {}
        assert_is_principal::<ServiceAccount>();
    }
    
    #[test]
    fn service_account_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<ServiceAccount>();
    }
    
    #[test]
    fn namespace_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<Namespace>();
    }
    
    #[test]
    fn create_user_action_implements_action_trait() {
        assert_eq!(CreateUserAction::name(), "create_user");
        let (principal, resource) = CreateUserAction::applies_to();
        assert_eq!(principal.to_string(), "User");
        assert_eq!(resource.to_string(), "User");
    }
    
    #[test]
    fn create_group_action_implements_action_trait() {
        assert_eq!(CreateGroupAction::name(), "create_group");
        let (principal, resource) = CreateGroupAction::applies_to();
        assert_eq!(principal.to_string(), "User");
        assert_eq!(resource.to_string(), "Group");
    }
}
</file>

<file path="crates/hodei-iam/tests/add_user_to_group_integration_test.rs">
/// Integration tests for add_user_to_group feature
///
/// These tests use in-memory repositories and coordinate between two aggregates

use hodei_iam::{
    features::{
        add_user_to_group::{self, dto::AddUserToGroupCommand},
        create_group::{self as create_group_feature, dto::CreateGroupCommand},
        create_user::{self as create_user_feature, dto::CreateUserCommand},
    },
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository},
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_add_user_to_group_success() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_cmd = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    };
    let user_view = create_user_uc.execute(user_cmd).await.unwrap();

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_cmd = CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    };
    let group_view = create_group_uc.execute(group_cmd).await.unwrap();

    // Act - Add user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert
    assert!(result.is_ok());

    // Verify that the user now belongs to the group
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 1);
    assert_eq!(users[0].groups()[0].to_string(), group_view.hrn);
}

#[tokio::test]
async fn test_add_user_to_group_idempotent() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user and group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Add user to group twice
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };

    let result1 = add_uc.execute(add_cmd.clone()).await;
    let result2 = add_uc.execute(add_cmd).await;

    // Assert - Both operations succeed
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // Verify that the user only has one group (no duplicates)
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users[0].groups().len(), 1);
}

#[tokio::test]
async fn test_add_user_to_nonexistent_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a user (no group)
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add user to nonexistent group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn,
        group_hrn: "hrn:hodei:iam:default:Group:nonexistent".to_string(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid group HRN") || err_msg.contains("Group not found"));
}

#[tokio::test]
async fn test_add_nonexistent_user_to_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a group (no user)
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add nonexistent user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: "hrn:hodei:iam:default:User:nonexistent".to_string(),
        group_hrn: group_view.hrn,
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid user HRN") || err_msg.contains("User not found"));
}
</file>

<file path="crates/hodei-iam/tests/create_user_integration_test.rs">
/// Integration tests for create_user feature
///
/// These tests use in-memory repositories to validate the complete vertical slice

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_create_user_success() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "Alice");
    assert_eq!(view.email, "alice@example.com");
    assert_eq!(view.groups.len(), 0); // No groups initially
    assert_eq!(view.tags.len(), 1);
    assert!(view.hrn.contains("User"));

    // Verify persistence
    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "Alice");
    assert_eq!(users[0].email, "alice@example.com");
}

#[tokio::test]
async fn test_create_multiple_users() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    // Act - Create multiple users
    let cmd1 = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };
    let cmd2 = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec!["developer".to_string()],
    };

    let result1 = use_case.execute(cmd1).await;
    let result2 = use_case.execute(cmd2).await;

    // Assert
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 2);
}
</file>

<file path="crates/hodei-iam/tests/integration_add_user_to_group_comprehensive_test.rs">
/// Comprehensive integration tests for add_user_to_group feature

use hodei_iam::{
    features::{
        add_user_to_group::{self, dto::AddUserToGroupCommand},
        create_group::{self as create_group_feature, dto::CreateGroupCommand},
        create_user::{self as create_user_feature, dto::CreateUserCommand},
    },
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository},
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_add_multiple_users_to_same_group() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple users and add them to the group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let users = vec!["Alice", "Bob", "Charlie"];

    for user_name in users {
        let user_view = create_user_uc.execute(CreateUserCommand {
            name: user_name.to_string(),
            email: format!("{}@test.com", user_name.to_lowercase()),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn,
            group_hrn: group_view.hrn.clone(),
        }).await;

        assert!(result.is_ok());
    }

    // Verify all users are in the group
    let all_users = user_repo.find_all().await.unwrap();
    for user in all_users {
        assert_eq!(user.groups().len(), 1);
        assert_eq!(user.groups()[0].to_string(), group_view.hrn);
    }
}

#[tokio::test]
async fn test_add_user_to_multiple_groups() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple groups
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let groups = vec!["developers", "designers", "managers"];

    for group_name in groups {
        let group_view = create_group_uc.execute(CreateGroupCommand {
            group_name: group_name.to_string(),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn,
        }).await;

        assert!(result.is_ok());
    }

    // Verify user is in all groups
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 3);
}

#[tokio::test]
async fn test_complex_user_group_relationships() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    // Create groups
    let dev_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    let ops_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "operations".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create users
    let alice = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let bob = create_user_uc.execute(CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Alice is in both groups
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn.clone(),
        group_hrn: dev_group.hrn.clone(),
    }).await.unwrap();

    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn,
        group_hrn: ops_group.hrn.clone(),
    }).await.unwrap();

    // Bob is only in developers
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: bob.hrn,
        group_hrn: dev_group.hrn,
    }).await.unwrap();

    // Verify relationships
    let all_users = user_repo.find_all().await.unwrap();
    let alice_user = all_users.iter().find(|u| u.name == "Alice").unwrap();
    let bob_user = all_users.iter().find(|u| u.name == "Bob").unwrap();

    assert_eq!(alice_user.groups().len(), 2);
    assert_eq!(bob_user.groups().len(), 1);
}
</file>

<file path="crates/hodei-iam/tests/unit_group_test.rs">
/// Unit tests for Group domain entity

use hodei_iam::Group;
use policies::shared::domain::hrn::Hrn;


#[test]
fn test_group_new_creates_empty_collections() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.name, "Developers");
    assert_eq!(group.tags.len(), 0);
    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_attach_policy_idempotent() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "policy1".into(),
    );

    // Attach policy twice
    group.attach_policy(policy_hrn.clone());
    group.attach_policy(policy_hrn.clone());

    // Should only have one policy
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy_hrn);
}

#[test]
fn test_group_detach_policy() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p1".into());
    let policy2 = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p2".into());

    group.attach_policy(policy1.clone());
    group.attach_policy(policy2.clone());
    assert_eq!(group.attached_policies().len(), 2);

    group.detach_policy(&policy1);
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy2);
}

#[test]
fn test_group_detach_nonexistent_policy_does_nothing() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p1".into());

    // Detach policy that doesn't exist
    group.detach_policy(&policy_hrn);

    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_name_getter() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.group_name(), "Developers");
}

#[test]
fn test_group_multiple_policies() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p1".into());
    let policy2 = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p2".into());
    let policy3 = Hrn::new("hodei".into(), "policies".into(), "default".into(), "Policy".into(), "p3".into());

    group.attach_policy(policy1);
    group.attach_policy(policy2);
    group.attach_policy(policy3);

    assert_eq!(group.attached_policies().len(), 3);
}
</file>

<file path="crates/hodei-iam/tests/unit_user_test.rs">
/// Unit tests for User domain entity

use hodei_iam::User;
use policies::shared::domain::hrn::Hrn;


#[test]
fn test_user_new_creates_empty_groups() {
    let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(hrn, "Alice".to_string(), "alice@test.com".to_string());

    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@test.com");
    assert_eq!(user.groups().len(), 0);
    assert_eq!(user.tags.len(), 0);
}

#[test]
fn test_user_add_to_group_idempotent() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "devs".into(),
    );

    // Add group twice
    user.add_to_group(group_hrn.clone());
    user.add_to_group(group_hrn.clone());

    // Should only have one group
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group_hrn);
}

#[test]
fn test_user_remove_from_group() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group1 = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "devs".into());
    let group2 = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "ops".into());

    user.add_to_group(group1.clone());
    user.add_to_group(group2.clone());
    assert_eq!(user.groups().len(), 2);

    user.remove_from_group(&group1);
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group2);
}

#[test]
fn test_user_remove_nonexistent_group_does_nothing() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new("hodei".into(), "IAM".into(), "default".into(), "Group".into(), "devs".into());

    // Remove group that doesn't exist
    user.remove_from_group(&group_hrn);

    assert_eq!(user.groups().len(), 0);
}

#[test]
fn test_user_email_getter() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(user_hrn, "Alice".to_string(), "alice@example.com".to_string());

    assert_eq!(user.email(), "alice@example.com");
}
</file>

<file path="crates/policies/src/features/policy_analysis/use_case.rs">
use super::dto::{AnalyzePoliciesRequest, AnalyzePoliciesResponse, RuleViolation};
use crate::shared::application::parallel::{evaluate_until_first, AuthScenario};
use cedar_policy::{
    Entities, EntityUid, Policy, PolicySet, Schema, SchemaFragment, ValidationMode, Validator,
};
use std::str::FromStr;

#[derive(Default)]
pub struct AnalyzePoliciesUseCase;

impl AnalyzePoliciesUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &AnalyzePoliciesRequest,
    ) -> Result<AnalyzePoliciesResponse, String> {
        // Build PolicySet once (fail fast on invalid)
        let mut pset = PolicySet::new();
        for (i, p) in req.policies.iter().enumerate() {
            let pol: Policy = p
                .parse()
                .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
            pset.add(pol)
                .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
        }

        // Heuristic + semantic checks
        let mut violations: Vec<RuleViolation> = Vec::new();

        // Optional: schema-based validation of policy set
        if let Some(s) = &req.schema {
            if let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s) {
                if let Ok(schema) = Schema::from_schema_fragments(vec![frag]) {
                    let v = Validator::new(schema);
                    let vr = v.validate(&pset, ValidationMode::default());
                    if !vr.validation_passed() {
                        for e in vr.validation_errors() {
                            violations.push(RuleViolation {
                                rule_id: "validator".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }
            }
        }

        for rule in &req.rules {
            match rule.kind.as_str() {
                "no_permit_without_mfa" => {
                    let principal = synth_euid("User", "synthetic").to_string();
                    let action = synth_euid("Action", "view").to_string();
                    let resource = synth_euid("Resource", "doc1").to_string();
                    let mut ctx_false = std::collections::HashMap::new();
                    ctx_false.insert("mfa".to_string(), serde_json::json!(false));
                    let scenarios = vec![
                        AuthScenario {
                            name: "mfa_false".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: Some(ctx_false),
                        },
                        AuthScenario {
                            name: "mfa_missing".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        },
                    ];
                    if let Some(out) = evaluate_until_first(
                        &pset,
                        &Entities::empty(),
                        scenarios,
                        None,
                        4,
                        8,
                        |o| o.allow,
                    )
                    .await?
                    {
                        violations.push(RuleViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Allow without strong auth: scenario='{}' P='{}' A='{}' R='{}'",
                                out.name, principal, action, resource
                            ),
                        });
                    }
                }
                "no_permit_without_condition" => {
                    let unconditioned = req.policies.iter().enumerate().any(|(_i, p)| {
                        let pol = p.to_lowercase();
                        pol.contains("permit(")
                            && !pol.contains(" when ")
                            && !pol.contains("unless ")
                    });
                    if unconditioned {
                        let principal = synth_euid("User", "u").to_string();
                        let action = synth_euid("Action", "a").to_string();
                        let resource = synth_euid("Resource", "r").to_string();
                        let scenarios = vec![AuthScenario {
                            name: "empty_ctx".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        }];
                        if let Some(out) = evaluate_until_first(
                            &pset,
                            &Entities::empty(),
                            scenarios,
                            None,
                            2,
                            4,
                            |o| o.allow,
                        )
                        .await?
                        {
                            violations.push(RuleViolation {
                                rule_id: rule.id.clone(),
                                message: format!(
                                    "Allow without condition: scenario='{}' P='{}' A='{}' R='{}'",
                                    out.name, principal, action, resource
                                ),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(AnalyzePoliciesResponse {
            passed: violations.is_empty(),
            violations,
        })
    }
}

fn synth_euid(etype: &str, name: &str) -> EntityUid {
    // Fall back to common types used in our playground
    let et = match etype {
        "User" | "user" => "User",
        "Action" | "action" => "Action",
        "Resource" | "resource" => "Resource",
        other => other,
    };
    EntityUid::from_str(&format!("{}::\"{}\"", et, name)).expect("valid synthetic euid")
}
</file>

<file path="crates/policies/src/features/policy_playground/use_case.rs">
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use crate::shared::application::parallel::{evaluate_scenarios_channel, AuthScenario};
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression, Schema, SchemaFragment, ValidationMode, Validator};

use super::dto::{
    AuthorizationDiagnostics, AuthorizationResult, Decision, EntityDefinition, EvaluationStatistics,
    PlaygroundRequest, PlaygroundResponse, PolicyValidationResult, SchemaValidationResult,
    ValidationError, ValidationWarning,
};

#[derive(Debug, thiserror::Error)]
pub enum PlaygroundError {
    #[error("invalid_request: {0}")]
    InvalidRequest(String),
    #[error("policy_parse_error: {0}")]
    PolicyParseError(String),
    #[error("euid_parse_error: {0}")]
    EuidParseError(String),
    #[error("request_build_error: {0}")]
    RequestError(String),
    #[error("schema_parse_error: {0}")]
    SchemaParseError(String),
    #[error("entity_parse_error: {0}")]
    EntityParseError(String),
}

// Helper: map serde_json::Value to RestrictedExpression (basic types)
fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: std::collections::BTreeMap<String, RestrictedExpression> = std::collections::BTreeMap::new();
            for (k, val) in map.iter() {
                if let Some(expr) = json_to_expr(val) {
                    rec.insert(k.clone(), expr);
                }
            }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

impl Default for PolicyPlaygroundUseCase {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PolicyPlaygroundUseCase;

impl PolicyPlaygroundUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &PlaygroundRequest,
    ) -> Result<PlaygroundResponse, PlaygroundError> {
        if req.policies.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one policy is required".to_string(),
            ));
        }
        if req.authorization_requests.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one authorization scenario is required".to_string(),
            ));
        }

        // Schema handling
        let schema_validation = self.validate_schema(&req.schema)?;

        // Parse and validate policies
        let (pset, policy_validation) = self.parse_and_validate_policies(&req.policies, &req.schema)?;

        // Build entities
        let entities = self.parse_entities(&req.entities)?;

        // Evaluate scenarios (parallel when >1)
        let mut results = Vec::with_capacity(req.authorization_requests.len());
        let mut total_time = 0u64;
        let mut allow_count = 0usize;

        if req.authorization_requests.len() == 1 {
            // Keep fast single-path
            let authorizer = Authorizer::new();
            let sc = &req.authorization_requests[0];
            let start = Instant::now();
            let principal = EntityUid::from_str(&sc.principal)
                .map_err(|e| PlaygroundError::EuidParseError(format!("principal: {}", e)))?;
            let action = EntityUid::from_str(&sc.action)
                .map_err(|e| PlaygroundError::EuidParseError(format!("action: {}", e)))?;
            let resource = EntityUid::from_str(&sc.resource)
                .map_err(|e| PlaygroundError::EuidParseError(format!("resource: {}", e)))?;
            let context = self.build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None)
                .map_err(|e| PlaygroundError::RequestError(e.to_string()))?;
            let resp = authorizer.is_authorized(&request, &pset, &entities);
            let decision = if resp.decision() == CedarDecision::Allow { allow_count += 1; Decision::Allow } else { Decision::Deny };
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;
            results.push(AuthorizationResult { scenario_name: sc.name.clone(), decision, determining_policies: vec![], evaluated_policies: vec![], diagnostics: AuthorizationDiagnostics { reasons, errors: vec![], info: vec![] }, evaluation_time_us: eval_time });
        } else {
            // Use shared parallel evaluator
            let auth_scenarios: Vec<AuthScenario> = req.authorization_requests
                .iter()
                .cloned()
                .map(|s| AuthScenario { name: s.name, principal: s.principal, action: s.action, resource: s.resource, context: s.context })
                .collect();
            let workers = 8usize;
            let buffer = 2 * workers;
            let (outcomes, _stats) = evaluate_scenarios_channel(&pset, &entities, auth_scenarios, None, workers, buffer)
                .await
                .map_err(|e| PlaygroundError::RequestError(e))?;
            for o in outcomes {
                if o.allow { allow_count += 1; }
                total_time += o.eval_time_us;
                results.push(AuthorizationResult {
                    scenario_name: o.name,
                    decision: if o.allow { Decision::Allow } else { Decision::Deny },
                    determining_policies: vec![],
                    evaluated_policies: vec![],
                    diagnostics: AuthorizationDiagnostics { reasons: o.reasons, errors: vec![], info: vec![] },
                    evaluation_time_us: o.eval_time_us,
                });
            }
        }

        // Stable order for determinism in tests
        results.sort_by(|a, b| a.scenario_name.cmp(&b.scenario_name));

        let statistics = EvaluationStatistics {
            total_scenarios: results.len(),
            allow_count,
            deny_count: results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if results.is_empty() { 0 } else { total_time / results.len() as u64 },
        };

        Ok(PlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: results,
            statistics,
        })
    }

    fn validate_schema(&self, schema_str: &Option<String>) -> Result<SchemaValidationResult, PlaygroundError> {
        if let Some(s) = schema_str {
            let (frag, _warnings) = SchemaFragment::from_cedarschema_str(s)
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            let _schema = Schema::from_schema_fragments(vec![frag])
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        } else {
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        }
    }

    fn parse_and_validate_policies(
        &self,
        policies: &[String],
        schema: &Option<String>,
    ) -> Result<(PolicySet, PolicyValidationResult), PlaygroundError> {
        let mut pset = PolicySet::new();
        let mut errors = Vec::new();
        let warnings = Vec::<ValidationWarning>::new();

        for (idx, pstr) in policies.iter().enumerate() {
            match pstr.parse::<Policy>() {
                Ok(pol) => {
                    if let Err(e) = pset.add(pol) {
                        errors.push(ValidationError { message: format!("add error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None });
                    }
                }
                Err(e) => errors.push(ValidationError { message: format!("parse error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None }),
            }
        }

        if errors.is_empty()
            && let Some(s) = schema
            && let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s)
            && let Ok(schema_obj) = Schema::from_schema_fragments(vec![frag])
        {
            let validator = Validator::new(schema_obj);
            let vr = validator.validate(&pset, ValidationMode::default());
            if !vr.validation_passed() {
                for e in vr.validation_errors() {
                    errors.push(ValidationError { message: e.to_string(), policy_id: None, line: None, column: None });
                }
            }
        }

        Ok((
            pset,
            PolicyValidationResult { is_valid: errors.is_empty(), errors, warnings, policies_count: policies.len() },
        ))
    }

    fn parse_entities(&self, defs: &[EntityDefinition]) -> Result<Entities, PlaygroundError> {
        if defs.is_empty() { return Ok(Entities::empty()); }
        let mut out = Vec::with_capacity(defs.len());
        for d in defs {
            let uid = EntityUid::from_str(&d.uid)
                .map_err(|e| PlaygroundError::EntityParseError(format!("{}", e)))?;
            let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
            for (k, v) in &d.attributes {
                if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
            }
            let mut parents: HashSet<EntityUid> = HashSet::new();
            for p in &d.parents {
                parents.insert(EntityUid::from_str(p).map_err(|e| PlaygroundError::EntityParseError(format!("parent: {}", e)))?);
            }
            let ent = Entity::new(uid, attrs, parents).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))?;
            out.push(ent);
        }
        Entities::from_entities(out, None).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))
    }

    fn build_context(&self, ctx: Option<&std::collections::HashMap<String, serde_json::Value>>) -> Context {
        let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
        if let Some(c) = ctx {
            for (k, v) in c {
                if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
            }
        }
        Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
    }
}
</file>

<file path="crates/policies/src/shared/application/parallel.rs">
use cedar_policy::{Authorizer, Context, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};

/// Scenario description compatible with Cedar
#[derive(Clone, Debug)]
pub struct AuthScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

/// mpsc-based pipeline with back-pressure and explicit worker count
pub async fn evaluate_scenarios_channel(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
) -> Result<(Vec<AuthOutcome>, ParallelStats), String> {
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clone shared inputs
    let policies = policies.clone();
    let entities = entities.clone();

    // Producer
    let scenarios_total = scenarios.len();
    tokio::spawn(async move {
        for sc in scenarios.into_iter() {
            if tx_in.send(sc).await.is_err() { break; }
        }
        // drop sender to close channel
    });

    // Workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            loop {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() {
                        for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
                    }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm.clone(), allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await {
                        Ok(o) => o,
                        Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] },
                    }
                } else { fut.await };
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in is moved into rx_arc
    drop(tx_out);

    // Collector
    let mut outcomes = Vec::new();
    let mut stats = ParallelStats { scenarios_total, ..Default::default() };
    while let Some(out) = rx_out.recv().await {
        if out.eval_time_us == 0 && out.reasons.iter().any(|r| r == "timeout") { stats.timeouts += 1; }
        stats.total_eval_time_us += out.eval_time_us;
        outcomes.push(out);
    }
    Ok((outcomes, stats))
}

/// Evaluate scenarios in parallel and return the first outcome matching the predicate.
/// Early-cancels remaining work using a shared AtomicBool.
pub async fn evaluate_until_first<F>(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
    predicate: F,
) -> Result<Option<AuthOutcome>, String>
where
    F: Fn(&AuthOutcome) -> bool + Send + Sync + 'static,
{
    let predicate = Arc::new(predicate);
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clones
    let policies = policies.clone();
    let entities = entities.clone();

    // producer
    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            for sc in scenarios.into_iter() {
                if cancel.load(Ordering::Relaxed) { break; }
                if tx_in.send(sc).await.is_err() { break; }
            }
        }
    });

    // workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            while !cancel.load(Ordering::Relaxed) {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() { for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } } }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm, allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await { Ok(o) => o, Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] } }
                } else { fut.await };
                if cancel.load(Ordering::Relaxed) { break; }
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in moved into rx_arc
    drop(tx_out);

    // collector
    while let Some(out) = rx_out.recv().await {
        if predicate.as_ref()(&out) {
            cancel.store(true, Ordering::Relaxed);
            return Ok(Some(out));
        }
    }
    Ok(None)
}

#[derive(Clone, Debug)]
pub struct AuthOutcome {
    pub name: String,
    pub allow: bool,
    pub eval_time_us: u64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ParallelStats {
    pub scenarios_total: usize,
    pub timeouts: usize,
    pub total_eval_time_us: u64,
}

pub fn build_policy_set(policies: &[String]) -> Result<PolicySet, String> {
    let mut pset = PolicySet::new();
    for (i, pstr) in policies.iter().enumerate() {
        let pol: Policy = pstr
            .parse()
            .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
        pset.add(pol)
            .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
    }
    Ok(pset)
}

pub fn build_entities(defs: &[(String, HashMap<String, serde_json::Value>, Vec<String>)]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for (uid_str, attrs_map, parents_vec) in defs {
        let uid = EntityUid::from_str(uid_str).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in attrs_map.iter() {
            if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
        }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in parents_vec.iter() { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

pub fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

pub async fn evaluate_scenarios_joinset(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    max_concurrency: usize,
) -> Result<Vec<AuthOutcome>, String> {
    let mut set: JoinSet<Result<AuthOutcome, String>> = JoinSet::new();
    let mut iter = scenarios.into_iter();

    // seed
    for _ in 0..max_concurrency {
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }

    let mut outcomes = Vec::new();
    while let Some(joined) = set.join_next().await {
        let out = match joined { Ok(r) => r, Err(e) => Err(e.to_string()) }?;
        outcomes.push(out);
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }
    Ok(outcomes)
}

fn spawn_eval(
    set: &mut JoinSet<Result<AuthOutcome, String>>,
    policies: PolicySet,
    entities: Entities,
    sc: AuthScenario,
    timeout_ms: Option<u64>,
) {
    set.spawn(async move {
        let authorizer = Authorizer::new();
        let principal = EntityUid::from_str(&sc.principal).map_err(|e| e.to_string())?;
        let action = EntityUid::from_str(&sc.action).map_err(|e| e.to_string())?;
        let resource = EntityUid::from_str(&sc.resource).map_err(|e| e.to_string())?;
        let context = {
            let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
            if let Some(ctx) = sc.context.as_ref() {
                for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
            }
            Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
        };
        let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;
        let name = sc.name.clone();
        let name_to = name.clone();
        let fut = async move {
            let start = std::time::Instant::now();
            let resp = authorizer.is_authorized(&request, &policies, &entities);
            let allow = resp.decision() == cedar_policy::Decision::Allow;
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time_us = start.elapsed().as_micros() as u64;
            Ok(AuthOutcome { name: name.clone(), allow, eval_time_us, reasons })
        };
        if let Some(ms) = timeout_ms {
            match timeout(Duration::from_millis(ms), fut).await {
                Ok(r) => r,
                Err(_elapsed) => Ok(AuthOutcome { name: name_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] }),
            }
        } else {
            fut.await
        }
    });
}
</file>

<file path="crates/policies/src/shared/infrastructure/mod.rs">
// Facade raíz del crate policies (estructura hexagonal interna)

// Infrastructure layer modules
pub mod surreal;
</file>

<file path="crates/policies/tests/delete_policy_integration_test.rs">
use cedar_policy::Policy;
use policies::features::delete_policy::di::make_delete_policy_use_case_mem;
use policies::features::delete_policy::dto::DeletePolicyCommand;
use policies::features::delete_policy::use_case::DeletePolicyError;

#[tokio::test]
async fn test_delete_policy_integration_success() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some());

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should succeed
    assert!(result.is_ok());
    assert!(result.unwrap());

    // Verify it's gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none());
}

#[tokio::test]
async fn test_delete_policy_integration_not_found() {
    // Arrange: Create use case with empty storage
    let (delete_uc, _engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete non-existent policy
    let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return NotFound error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::NotFound(id)) => {
            assert_eq!(id, "nonexistent_policy_id");
        }
        _ => panic!("Expected NotFound error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_invalid_id() {
    // Arrange: Create use case
    let (delete_uc, _engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete with empty ID
    let cmd = DeletePolicyCommand::new("");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return InvalidCommand error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::InvalidCommand(_)) => {}
        _ => panic!("Expected InvalidCommand error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_idempotent() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some(), "Policy should exist before deletion");

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;
    assert!(result.is_ok(), "First deletion should succeed");

    // Assert: Policy is gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none(), "Policy should be deleted");

    // Act: Try to delete again
    let cmd2 = DeletePolicyCommand::new(policy_id.clone());
    let result2 = delete_uc.execute(&cmd2).await;

    // Assert: Should return NotFound error
    assert!(
        result2.is_err(),
        "Second deletion should fail with NotFound"
    );
    match result2 {
        Err(DeletePolicyError::NotFound(_)) => {}
        _ => panic!("Expected NotFound error on second deletion"),
    }
}
</file>

<file path="crates/policies/tests/hodei_entity_test.rs">
//! Test to verify the HodeiEntity implementation with RestrictedExpression

use cedar_policy::{Entity, EntityUid, RestrictedExpression, Schema, SchemaFragment};
use std::collections::HashMap;
use std::str::FromStr;

/// Example implementation of HodeiEntity for testing
#[derive(Debug)]
struct TestUser {
    id: String,
    name: String,
    email: String,
    groups: Vec<String>,
    tags: Vec<String>,
}

impl TestUser {
    fn new(
        id: String,
        name: String,
        email: String,
        groups: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            name,
            email,
            groups,
            tags,
        }
    }

    fn euid(&self) -> EntityUid {
        EntityUid::from_str(&format!("User::\"{}\"", self.id)).unwrap()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );

        // For collections, we use new_set
        let group_expressions: Vec<RestrictedExpression> = self
            .groups
            .iter()
            .map(|group| RestrictedExpression::new_string(group.clone()))
            .collect();
        attrs.insert(
            "groups".to_string(),
            RestrictedExpression::new_set(group_expressions),
        );

        let tag_expressions: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|tag| RestrictedExpression::new_string(tag.clone()))
            .collect();
        attrs.insert(
            "tags".to_string(),
            RestrictedExpression::new_set(tag_expressions),
        );

        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        // In a real implementation, this would convert group names to EntityUids
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hodei_entity_implementation() {
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let attributes = user.attributes();
        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("email"));
        assert!(attributes.contains_key("groups"));
        assert!(attributes.contains_key("tags"));

        let entity = Entity::new(
            user.euid(),
            attributes,
            user.parents().into_iter().collect(),
        );

        assert!(entity.is_ok());
    }

    #[test]
    fn test_cedar_integration() -> Result<(), Box<dyn std::error::Error>> {
        // Create a simple schema
        let schema_str = r#"
        entity User {
            name: String,
            email: String,
            groups: Set<String>,
            tags: Set<String>
        };
        
        action access appliesTo {
            principal: User,
            resource: User
        };
        "#;

        let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
        let _schema = Schema::from_schema_fragments([schema_fragment])?;

        // Create a user entity
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let entity = Entity::new(
            user.euid(),
            user.attributes(),
            user.parents().into_iter().collect(),
        )?;

        // Validate that the entity conforms to the schema
        assert_eq!(entity.uid().to_string(), r#"User::"alice""#);

        Ok(())
    }
}
</file>

<file path="crates/policies/tests/list_policies_integration_test.rs">
use cedar_policy::Policy;
use policies::features::list_policies::di::make_list_policies_use_case_mem;
use policies::features::list_policies::dto::ListPoliciesQuery;

#[tokio::test]
async fn test_list_policies_integration_empty() {
    // Arrange: Create use case with empty storage
    let (list_uc, _engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return empty list
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert_eq!(policies.len(), 0);
}

#[tokio::test]
async fn test_list_policies_integration_with_data() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy using the store
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return at least 1 policy
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert!(
        policies.len() >= 1,
        "Expected at least 1 policy, got {}",
        policies.len()
    );
}

#[tokio::test]
async fn test_list_policies_integration_after_create_and_delete() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it's listed
    let query = ListPoliciesQuery::new();
    let policies = list_uc.execute(&query).await.expect("list policies");
    assert_eq!(policies.len(), 1);

    // Delete the policy
    engine
        .store
        .remove_policy(&policy_id)
        .await
        .expect("remove policy");

    // Act: List policies again
    let policies_after = list_uc
        .execute(&query)
        .await
        .expect("list policies after delete");

    // Assert: Should be empty
    assert_eq!(policies_after.len(), 0);
}
</file>

<file path="crates/policies/tests/schema_rendering_final_test.rs">
use async_trait::async_trait;
use cedar_policy::{EntityTypeName, EntityUid, Policy, PolicySet, RestrictedExpression, Schema};
/// Tests para verificar el rendering final del schema generado por el EngineBuilder
///
/// Estos tests registran diferentes tipos de entidades y acciones para validar
/// que el schema final se genera correctamente con namespaces, atributos y relaciones.
/// Usan validación de Cedar como fuente principal de verdad.

use policies::shared::application::EngineBuilder;
use policies::shared::domain::ports::{
    Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource,
    StorageError,
};
use policies::shared::Hrn;
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

// ============================================================================
// Mock Storage
// ============================================================================

struct MockStorage;

#[async_trait]
impl PolicyStorage for MockStorage {
    async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        Ok(vec![])
    }
}

// ============================================================================
// Mock IAM Entities (Principals)
// ============================================================================

struct IamUser {
    hrn: Hrn,
}

impl HodeiEntityType for IamUser {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("email", AttributeType::Primitive("String")),
            ("name", AttributeType::Primitive("String")),
            ("active", AttributeType::Primitive("Bool")),
            ("roles", AttributeType::Set(Box::new(AttributeType::Primitive("String")))),
        ]
    }
}

impl HodeiEntity for IamUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamUser {}

struct IamGroup {
    hrn: Hrn,
}

impl HodeiEntityType for IamGroup {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("description", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for IamGroup {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamGroup {}

// ============================================================================
// Mock Artifact Entities (Resources)
// ============================================================================

struct ArtifactPackage {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactPackage {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Package"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("version", AttributeType::Primitive("String")),
            ("type", AttributeType::Primitive("String")),
            ("size", AttributeType::Primitive("Long")),
            ("tags", AttributeType::Set(Box::new(AttributeType::Primitive("String")))),
        ]
    }
}

impl HodeiEntity for ArtifactPackage {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactPackage {}

struct ArtifactRepository {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactRepository {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Repository"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("visibility", AttributeType::Primitive("String")),
            ("ownerId", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for ArtifactRepository {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactRepository {}

// ============================================================================
// Mock Actions
// ============================================================================

struct ReadPackageAction;

impl Action for ReadPackageAction {
    fn name() -> &'static str {
        "ReadPackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct WritePackageAction;

impl Action for WritePackageAction {
    fn name() -> &'static str {
        "WritePackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct ManageRepositoryAction;

impl Action for ManageRepositoryAction {
    fn name() -> &'static str {
        "ManageRepository"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::Group").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Repository").expect("Valid resource type");
        (principal, resource)
    }
}

// ============================================================================
// Helper para renderizar schema
// ============================================================================

fn render_schema(schema: &Schema) -> String {
    format!("{:#?}", schema)
}

fn print_schema_details(schema: &Schema, title: &str) {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║ {:<62} ║", title);
    println!("╚════════════════════════════════════════════════════════════════╝");

    let schema_str = render_schema(schema);

    println!("\n📋 Schema Debug Output:");
    println!("{}", schema_str);

    println!("\n✅ Schema built successfully!");
    println!("   - Entity types, actions, and relationships are properly defined");
    println!("   - Namespaces are correctly structured");
    println!("   - All fragments were merged without conflicts\n");
}

/// Valida que una política es válida contra el schema usando Cedar
fn validate_policy_against_schema(schema: &Schema, policy_str: &str) -> Result<(), String> {
    let policy: Policy = policy_str.parse()
        .map_err(|e| format!("Failed to parse policy: {}", e))?;

    let mut policy_set = PolicySet::new();
    policy_set.add(policy)
        .map_err(|e| format!("Failed to add policy to set: {}", e))?;

    let validator = cedar_policy::Validator::new(schema.clone());
    let validation_result = validator.validate(&policy_set, cedar_policy::ValidationMode::default());

    if validation_result.validation_passed() {
        Ok(())
    } else {
        let errors: Vec<String> = validation_result.validation_errors()
            .map(|e| format!("{:?}", e))
            .collect();
        Err(format!("Validation failed: {:?}", errors))
    }
}

/// Verifica que el schema contiene los componentes esperados usando validación de políticas
fn assert_schema_contains_entities_and_actions(schema: &Schema, expected_components: &[&str]) {
    for component in expected_components {
        let test_policy = if component.starts_with("Action::") {
            // Para una acción como "Action::"ReadPackage"", creamos una política que la use
            format!("permit(principal, action == {}, resource);", component)
        } else if component.contains("::") {
            // Para una entidad como "Iam::User", creamos una política que la use en una condición 'is'
            format!("permit(principal, action, resource) when {{ principal is {} }};", component)
        } else {
            // Ignorar componentes no reconocidos
            continue;
        };

        validate_policy_against_schema(schema, &test_policy)
            .unwrap_or_else(|e| panic!("Schema validation failed for component '{}': {}\nGenerated policy: {}", component, e, test_policy));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_schema_with_single_principal_and_resource() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Single Principal and Resource ===");
    println!("{}", schema_str);
    println!("=================================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Action::\"ReadPackage\""]
    );

    let test_policy = r#"
        permit(
            principal == Iam::User::"alice",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-123"
        );
    "#;

    validate_policy_against_schema(schema, test_policy)
        .expect("Policy should be valid against schema");
}

#[tokio::test]
async fn test_schema_with_multiple_principals() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Principals ===");
    println!("{}", schema_str);
    println!("========================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Iam::Group", "Artifact::Package", "Action::\"ReadPackage\""]
    );

    let user_policy = r#"
        permit(
            principal == Iam::User::"bob",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-456"
        );
    "#;
    validate_policy_against_schema(schema, user_policy)
        .expect("User policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_resources() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Resources ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\""]
    );

    let package_policy = r#"
        permit(
            principal == Iam::User::"charlie",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-789"
        );
    "#;
    validate_policy_against_schema(schema, package_policy)
        .expect("Package policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_actions() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Actions ===");
    println!("{}", schema_str);
    println!("=====================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Iam::Group", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\"", "Action::\"WritePackage\"", "Action::\"ManageRepository\""]
    );

    let read_policy = r#"
        permit(
            principal == Iam::User::"dave",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-read"
        );
    "#;
    validate_policy_against_schema(schema, read_policy)
        .expect("Read policy should be valid");

    let write_policy = r#"
        permit(
            principal == Iam::User::"eve",
            action == Action::"WritePackage",
            resource == Artifact::Package::"pkg-write"
        );
    "#;
    validate_policy_against_schema(schema, write_policy)
        .expect("Write policy should be valid");

    let manage_policy = r#"
        permit(
            principal == Iam::Group::"admins",
            action == Action::"ManageRepository",
            resource == Artifact::Repository::"repo-main"
        );
    "#;
    validate_policy_against_schema(schema, manage_policy)
        .expect("Manage policy should be valid");
}

#[tokio::test]
async fn test_schema_with_complex_attributes() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()      // <-- Recurso que faltaba
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()       // <-- Acción que faltaba
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Complex Attributes ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\""]
    );

    let complex_policy = r#"
        permit(
            principal == Iam::User::"frank",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-complex"
        ) when {
            principal.active == true
        };
    "#;
    validate_policy_against_schema(schema, complex_policy)
        .expect("Complex policy should be valid");
}

#[tokio::test]
async fn test_complete_schema_rendering() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();

    // Registrar todos los principals
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup");

    // Registrar todos los resources
    builder
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository");

    // Registrar todas las acciones
    builder
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;

    print_schema_details(schema, "COMPLETE SCHEMA RENDERING TEST");

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Schema Components Registered:                                ║");
    println!("║  - Principals: IamUser, IamGroup                              ║");
    println!("║  - Resources: ArtifactPackage, ArtifactRepository             ║");
    println!("║  - Actions: ReadPackage, WritePackage, ManageRepository       ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User", "Iam::Group",
            "Artifact::Package", "Artifact::Repository",
            "Action::\"ReadPackage\"", "Action::\"WritePackage\"", "Action::\"ManageRepository\""
        ]
    );

    let policies = vec![
        r#"permit(principal == Iam::User::"admin", action == Action::"WritePackage", resource == Artifact::Package::"critical-pkg");"#,
        r#"permit(principal == Iam::Group::"devops", action == Action::"ManageRepository", resource == Artifact::Repository::"prod-repo");"#,
        r#"permit(principal == Iam::User::"reader", action == Action::"ReadPackage", resource == Artifact::Package::"public-pkg") when { principal.active == true };"#,
    ];

    for (idx, policy_str) in policies.iter().enumerate() {
        validate_policy_against_schema(schema, policy_str)
            .unwrap_or_else(|e| panic!("Policy {} should be valid: {}", idx, e));
    }

    println!("\n✅ All {} policies validated successfully against the complete schema!", policies.len());
}

#[tokio::test]
async fn test_empty_schema() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let builder = EngineBuilder::new();
    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Empty Schema (No Registrations) ===");
    println!("{}", schema_str);
    println!("========================================\n");

    let iam_pattern = Regex::new(r"namespace\s+Iam").expect("Valid regex");
    let artifact_pattern = Regex::new(r"namespace\s+Artifact").expect("Valid regex");

    assert!(!iam_pattern.is_match(&schema_str), "Empty schema should not contain Iam namespace");
    assert!(!artifact_pattern.is_match(&schema_str), "Empty schema should not contain Artifact namespace");

    let invalid_policy = r#"
        permit(
            principal == Iam::User::"test",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"test"
        );
    "#;

    let result = validate_policy_against_schema(schema, invalid_policy);
    assert!(result.is_err(), "Policy should fail validation against empty schema");
    println!("✅ Policy correctly failed validation against empty schema: {:?}", result.err());
}
</file>

<file path="crates/policies/tests/shared_parallel_test.rs">
use policies::shared::application::parallel::{
    build_entities, build_policy_set, evaluate_scenarios_channel, evaluate_until_first, AuthScenario
};

#[tokio::test]
async fn channel_evaluates_multiple_scenarios() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.mfa == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let scenarios = vec![
        AuthScenario { name: "s1".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
        AuthScenario { name: "s2".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
    ];

    let (outcomes, stats) = evaluate_scenarios_channel(&pset, &ents, scenarios, None, 4, 8).await.expect("run");
    assert_eq!(outcomes.len(), 2);
    assert_eq!(stats.scenarios_total, 2);
    assert!(outcomes.iter().all(|o| o.allow));
}

#[tokio::test]
async fn until_first_returns_on_first_allow() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.allowed == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let mut ctx_deny = std::collections::HashMap::new();
    ctx_deny.insert("allowed".to_string(), serde_json::json!(false));
    let mut ctx_allow = std::collections::HashMap::new();
    ctx_allow.insert("allowed".to_string(), serde_json::json!(true));

    let scenarios = vec![
        AuthScenario { name: "deny".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_deny) },
        AuthScenario { name: "allow".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_allow) },
    ];

    let first = evaluate_until_first(&pset, &ents, scenarios, None, 2, 4, |o| o.allow).await.expect("run");
    assert!(first.is_some());
    assert_eq!(first.unwrap().name, "allow");
}
</file>

<file path="crates/hodei-iam/tests/integration_create_user_comprehensive_test.rs">
/// Comprehensive integration tests for create_user feature

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_create_user_with_valid_email() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.groups.len(), 0);
    assert_eq!(view.tags.len(), 1);
}

#[tokio::test]
async fn test_create_user_multiple_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Jane Smith".to_string(),
        email: "jane@example.com".to_string(),
        tags: vec!["developer".to_string(), "senior".to_string(), "fullstack".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"developer".to_string()));
    assert!(view.tags.contains(&"senior".to_string()));
}

#[tokio::test]
async fn test_create_user_no_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);
}

#[tokio::test]
async fn test_create_user_hrn_format() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await.unwrap();

    // Verify HRN format: hrn:partition:service::account_id:resource_type/resource_id
    assert!(result.hrn.starts_with("hrn:"), "HRN should start with 'hrn:'");
    assert!(result.hrn.contains(":iam:"), "HRN should contain service 'iam' in lowercase");
    assert!(result.hrn.contains(":User/"), "HRN should contain resource_type 'User' followed by '/'");
}

#[tokio::test]
async fn test_create_user_unique_ids() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Same Name".to_string(),
        email: "same@example.com".to_string(),
        tags: vec![],
    };

    let result1 = use_case.execute(command.clone()).await.unwrap();
    let result2 = use_case.execute(command.clone()).await.unwrap();

    // Even with same data, HRNs should be different (UUID)
    assert_ne!(result1.hrn, result2.hrn);
}

#[tokio::test]
async fn test_create_users_batch() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let users = vec![
        ("Alice", "alice@test.com"),
        ("Bob", "bob@test.com"),
        ("Charlie", "charlie@test.com"),
    ];

    for (name, email) in users {
        let command = CreateUserCommand {
            name: name.to_string(),
            email: email.to_string(),
            tags: vec![],
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    let all_users = repo.find_all().await.unwrap();
    assert_eq!(all_users.len(), 3);
}
</file>

<file path="crates/hodei-iam/tests/unit_hrn_constructor_test.rs">
/// Unit tests for Hrn constructor with HodeiEntityType

use hodei_iam::{Group, User};
use policies::shared::domain::hrn::Hrn;


#[test]
fn test_hrn_for_entity_type_user() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "user123".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam");  // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "User");
    assert_eq!(hrn.resource_id, "user123");
}

#[test]
fn test_hrn_for_entity_type_group() {
    let hrn = Hrn::for_entity_type::<Group>(
        "hodei".to_string(),
        "default".to_string(),
        "group456".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam");  // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "Group");
    assert_eq!(hrn.resource_id, "group456");
}

#[test]
fn test_hrn_for_entity_type_to_string() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "account1".to_string(),
        "alice".to_string(),
    );

    let hrn_str = hrn.to_string();
    assert!(hrn_str.contains(":iam:"));  // service is lowercase in HRN string
    assert!(hrn_str.contains(":User/"));  // resource_type followed by /
    assert!(hrn_str.contains("alice"));
}

#[test]
fn test_hrn_for_entity_type_euid() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "bob".to_string(),
    );

    let euid = hrn.euid();
    let euid_str = format!("{}", euid);

    assert!(euid_str.contains("Iam::User"));  // Cedar namespace is PascalCase
    assert!(euid_str.contains("bob"));
}
</file>

<file path="crates/policies/src/features/delete_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct DeletePolicyCommand {
    pub policy_id: String,
}

impl DeletePolicyCommand {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), DeletePolicyValidationError> {
        if self.policy_id.trim().is_empty() {
            return Err(DeletePolicyValidationError::EmptyPolicyId);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeletePolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
}
</file>

<file path="crates/policies/src/features/delete_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/get_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct GetPolicyQuery {
    pub policy_id: String,
}

impl GetPolicyQuery {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), GetPolicyValidationError> {
        if self.policy_id.trim().is_empty() {
            return Err(GetPolicyValidationError::EmptyPolicyId);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
}
</file>

<file path="crates/policies/src/features/get_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/list_policies/dto.rs">
#[derive(Debug, Clone)]
pub struct ListPoliciesQuery {
    /// Pagination: number of items to skip
    pub offset: Option<usize>,
    /// Pagination: maximum number of items to return
    pub limit: Option<usize>,
    /// Filter: only return policies with IDs containing this string
    pub filter_id: Option<String>,
}

impl ListPoliciesQuery {
    pub fn new() -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: None,
        }
    }

    pub fn with_pagination(offset: usize, limit: usize) -> Self {
        Self {
            offset: Some(offset),
            limit: Some(limit),
            filter_id: None,
        }
    }

    pub fn with_filter(filter_id: String) -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: Some(filter_id),
        }
    }

    pub fn validate(&self) -> Result<(), ListPoliciesValidationError> {
        // Validate limit is reasonable
        if let Some(limit) = self.limit {
            if limit == 0 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit must be greater than 0".to_string(),
                ));
            }
            if limit > 1000 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit cannot exceed 1000".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesValidationError {
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
}
</file>

<file path="crates/policies/src/features/list_policies/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/policy_playground_traces/use_case.rs">
use super::dto::{TracedAuthorizationResult, TracedPlaygroundOptions, TracedPlaygroundResponse};
use crate::features::policy_playground::dto as base;
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use tokio::task::JoinSet;

pub struct TracedPlaygroundUseCase;

impl TracedPlaygroundUseCase {
    pub fn new() -> Self { Self }

    pub async fn execute(
        &self,
        options: &TracedPlaygroundOptions,
        base_req: &base::PlaygroundRequest,
        base_uc: &crate::features::policy_playground::use_case::PolicyPlaygroundUseCase,
    ) -> Result<TracedPlaygroundResponse, String> {
        if !options.include_policy_traces {
            // Fast path: no traces, just call base
            let result = base_uc.execute(base_req).await.map_err(|e| e.to_string())?;
            let wrapped: Vec<TracedAuthorizationResult> = result
                .authorization_results
                .into_iter()
                .map(|base_res| TracedAuthorizationResult { base: base_res, determining_policies: None, evaluated_policies: None })
                .collect();
            return Ok(TracedPlaygroundResponse {
                policy_validation: result.policy_validation,
                schema_validation: result.schema_validation,
                authorization_results: wrapped,
                statistics: result.statistics,
            });
        }

        // Heuristic path: DO NOT call base_uc to avoid ID conflicts; replicate minimal logic
        // Parse all policies together as a single PolicySet to get consistent IDs
        let mut policy_set_str = String::new();
        for pstr in base_req.policies.iter() {
            policy_set_str.push_str(pstr.trim());
            policy_set_str.push_str("\n\n");
        }
        
        // Parse the entire PolicySet at once
        let pset_parsed = PolicySet::from_str(&policy_set_str)
            .map_err(|e| format!("failed to parse policy set: {}", e))?;
        
        // Extract policies with their Cedar-assigned IDs
        let mut policies: Vec<(String, Policy)> = Vec::with_capacity(base_req.policies.len());
        for p in pset_parsed.policies() {
            let id = p.id().to_string();
            policies.push((id, p.clone()));
        }

        // Minimal validation result (no schema/policy validation for traces mode)
        let policy_validation = base::PolicyValidationResult {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
            policies_count: policies.len(),
        };
        let schema_validation = base::SchemaValidationResult {
            is_valid: true,
            errors: vec![],
            entity_types_count: 0,
            actions_count: 0,
        };

        // Build Entities from request
        let entities = build_entities(&base_req.entities)?;

        // Authorizer
        let authorizer = Authorizer::new();

        // For each scenario, compute determining policies by removal (parallel per policy)
        let mut traced_results: Vec<TracedAuthorizationResult> = Vec::with_capacity(base_req.authorization_requests.len());
        let mut total_time: u64 = 0;
        let mut allow_count: usize = 0;
        for sc in &base_req.authorization_requests {

            let principal = EntityUid::from_str(&sc.principal).map_err(|e| format!("principal: {}", e))?;
            let action = EntityUid::from_str(&sc.action).map_err(|e| format!("action: {}", e))?;
            let resource = EntityUid::from_str(&sc.resource).map_err(|e| format!("resource: {}", e))?;
            let context = build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;

            let start = std::time::Instant::now();

            // Build full PolicySet - use the parsed one directly to preserve IDs
            let pset_all = pset_parsed.clone();

            // Baseline
            let baseline = authorizer.is_authorized(&request, &pset_all, &entities);
            let baseline_allow = baseline.decision() == CedarDecision::Allow;
            if baseline_allow { allow_count += 1; }

            // Parallel removal
            let mut set: JoinSet<(String, bool)> = JoinSet::new();
            let policy_strings = base_req.policies.clone(); // Keep original strings
            for (i, (pol_id, _)) in policies.iter().enumerate() {
                let pol_id_cloned = pol_id.clone();
                let policy_strings_clone = policy_strings.clone();
                let entities_clone = entities.clone();
                let sc_principal_c = sc.principal.clone();
                let sc_action_c = sc.action.clone();
                let sc_resource_c = sc.resource.clone();
                let sc_context_c = sc.context.clone();
                set.spawn(async move {
                    // Rebuild PolicySet without policy i
                    let mut pset_str = String::new();
                    for (j, pstr) in policy_strings_clone.iter().enumerate() {
                        if i != j {
                            pset_str.push_str(pstr.trim());
                            pset_str.push_str("\n\n");
                        }
                    }
                    let pset = PolicySet::from_str(&pset_str).unwrap_or_else(|_| PolicySet::new());
                    
                    // Recreate request
                    let principal = EntityUid::from_str(&sc_principal_c).unwrap();
                    let action = EntityUid::from_str(&sc_action_c).unwrap();
                    let resource = EntityUid::from_str(&sc_resource_c).unwrap();
                    let context = build_context(sc_context_c.as_ref());
                    let request = Request::new(principal, action, resource, context, None).unwrap();
                    let a = Authorizer::new();
                    let resp = a.is_authorized(&request, &pset, &entities_clone);
                    let allow = resp.decision() == CedarDecision::Allow;
                    (pol_id_cloned, allow)
                });
            }

            let mut determining: Vec<String> = Vec::new();
            while let Some(joined) = set.join_next().await {
                if let Ok((id, allow)) = joined { if allow != baseline_allow { determining.push(id); } }
            }

            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;

            let base_result = base::AuthorizationResult {
                scenario_name: sc.name.clone(),
                decision: if baseline_allow { base::Decision::Allow } else { base::Decision::Deny },
                determining_policies: vec![],
                evaluated_policies: vec![],
                diagnostics: base::AuthorizationDiagnostics { reasons: vec![], errors: vec![], info: vec![] },
                evaluation_time_us: eval_time,
            };

            traced_results.push(TracedAuthorizationResult {
                base: base_result,
                determining_policies: Some(determining),
                evaluated_policies: None,
            });
        }

        let statistics = base::EvaluationStatistics {
            total_scenarios: traced_results.len(),
            allow_count,
            deny_count: traced_results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if traced_results.is_empty() { 0 } else { total_time / traced_results.len() as u64 },
        };

        Ok(TracedPlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: traced_results,
            statistics,
        })
    }
}

fn build_entities(defs: &[base::EntityDefinition]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for d in defs {
        let uid = EntityUid::from_str(&d.uid).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in &d.attributes { if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); } }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in &d.parents { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

fn build_context(ctx: Option<&HashMap<String, serde_json::Value>>) -> Context {
    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
    if let Some(c) = ctx {
        for (k, v) in c {
            if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
        }
    }
    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
}

fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn determining_policy_includes_forbid_group() {
        // Policies: forbid admins; permit all (no explicit IDs, will be auto-assigned)
        let req = base::PlaygroundRequest {
            policies: vec![
                "forbid(principal in Group::\"admins\", action, resource);".to_string(),
                "permit(principal, action, resource);".to_string(),
            ],
            schema: None,
            entities: vec![
                base::EntityDefinition { uid: "User::\"alice\"".to_string(), attributes: Default::default(), parents: vec!["Group::\"admins\"".to_string()] },
                base::EntityDefinition { uid: "Group::\"admins\"".to_string(), attributes: Default::default(), parents: vec![] },
            ],
            authorization_requests: vec![ base::AuthorizationScenario {
                name: "alice-deny".to_string(),
                principal: "User::\"alice\"".to_string(),
                action: "Action::\"view\"".to_string(),
                resource: "Resource::\"doc1\"".to_string(),
                context: None,
            }],
            options: None,
        };

        let base_uc = crate::features::policy_playground::use_case::PolicyPlaygroundUseCase::default();
        let traced_uc = TracedPlaygroundUseCase::new();
        let opts = TracedPlaygroundOptions { include_policy_traces: true };
        let res = traced_uc.execute(&opts, &req, &base_uc).await.unwrap();
        let det = &res.authorization_results[0].determining_policies;
        
        assert!(det.as_ref().unwrap().len() >= 1);
        // The forbid policy is determining (removing it changes decision from Deny to Allow)
        // Cedar assigns IDs automatically (policy0, policy1, etc.)
        // The determining policy should be one of them (either policy0 or policy1 depending on parse order)
        let determining_policies = det.as_ref().unwrap();
        assert!(
            determining_policies.contains(&"policy0".to_string()) || 
            determining_policies.contains(&"policy1".to_string()),
            "Expected either policy0 or policy1 in determining policies, but got: {:?}",
            determining_policies
        );
    }
}
</file>

<file path="crates/policies/src/features/update_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct UpdatePolicyCommand {
    pub policy_id: String,
    pub new_policy_content: String,
}

impl UpdatePolicyCommand {
    pub fn new(policy_id: String, new_policy_content: String) -> Self {
        Self {
            policy_id,
            new_policy_content,
        }
    }

    pub fn validate(&self) -> Result<(), UpdatePolicyValidationError> {
        // Validar ID no vacío
        if self.policy_id.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyId);
        }

        // Validar contenido no vacío
        if self.new_policy_content.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyContent);
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
    #[error("policy content cannot be empty")]
    EmptyPolicyContent,
}
</file>

<file path="crates/policies/src/features/update_policy/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/shared/application/di_helpers.rs">
use crate::shared::application::{AuthorizationEngine, EngineBuilder, PolicyStore};
use crate::shared::domain::PolicyStorage;
use crate::shared::infrastructure::surreal::SurrealMemStorage;
use anyhow::Result;
/// Centralized DI helpers to avoid code duplication across features
/// 
/// This module provides reusable functions for building engines and storage,
/// allowing features to focus on their specific use case construction.

use std::sync::Arc;

#[cfg(feature = "embedded")]
use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses in-memory storage (default dev/test)
pub async fn build_engine_mem<F>(configurator: F) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealMemStorage::new("policies", "policies").await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses embedded storage (RocksDB)
#[cfg(feature = "embedded")]
pub async fn build_engine_embedded<F>(
    path: &str,
    configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealEmbeddedStorage::new("policies", "policies", path).await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// No-op configurator - creates an engine with NO entities registered (domain agnostic)
pub fn no_entities_configurator(builder: EngineBuilder) -> Result<EngineBuilder> {
    Ok(builder)
}

/// Test helpers module - provides reusable test entities and configurators
/// Available in both test and non-test builds for integration tests and examples
pub mod test_helpers {
    use super::*;
    use crate::shared::domain::ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
    use crate::shared::Hrn;
    use cedar_policy::{EntityTypeName, EntityUid, RestrictedExpression};
    use std::collections::HashMap;
    use std::str::FromStr;

    // Test Principal type
    pub struct TestPrincipal {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestPrincipal {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Principal"
        }
        fn is_principal_type() -> bool {
            true
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("email", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestPrincipal {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Principal for TestPrincipal {}

    // Test Resource type
    pub struct TestResource {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestResource {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Resource"
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("name", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestResource {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Resource for TestResource {}

    // Test Action
    pub struct TestAccessAction;

    impl Action for TestAccessAction {
        fn name() -> &'static str {
            "access"
        }
        fn applies_to() -> (EntityTypeName, EntityTypeName) {
            let principal_type = EntityTypeName::from_str("Test::Principal")
                .expect("Valid principal type");
            let resource_type = EntityTypeName::from_str("Test::Resource")
                .expect("Valid resource type");
            (principal_type, resource_type)
        }
    }

    /// Configurator for tests - registers basic test entities and actions
    pub fn test_entities_configurator(mut builder: EngineBuilder) -> Result<EngineBuilder> {
        builder
            .register_principal::<TestPrincipal>()?
            .register_resource::<TestResource>()?
            .register_action::<TestAccessAction>()?;
        Ok(builder)
    }
}
</file>

<file path="crates/policies/src/shared/application/store.rs">
use crate::domain::{PolicyStorage, StorageError};
use cedar_policy::{Policy, PolicySet, Schema, Validator};
use std::sync::Arc;

#[derive(Clone)]
pub struct PolicyStore {
    storage: Arc<dyn PolicyStorage>,
    validator: Validator,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PolicyStorage;
    use async_trait::async_trait;

    #[derive(Clone)]
    struct DummyStorage;

    #[async_trait]
    impl PolicyStorage for DummyStorage {
        async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
            Ok(None)
        }
        async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
            Ok(vec![])
        }
    }

    fn minimal_schema() -> Arc<Schema> {
        let minimal_schema = r#"
        entity Principal { };
        action access appliesTo {
            principal: Principal,
            resource: Principal
        };
        "#;
        let (fragment, _) = cedar_policy::SchemaFragment::from_cedarschema_str(minimal_schema)
            .expect("minimal schema valid");
        Arc::new(Schema::from_schema_fragments(vec![fragment]).expect("schema build"))
    }

    #[tokio::test]
    async fn get_current_policy_set_returns_empty_with_dummy_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let pset = store.get_current_policy_set().await.expect("policy set");
        // Rendering should be possible
        let rendered = pset.to_cedar();
        assert!(rendered.is_some());
    }

    #[tokio::test]
    async fn remove_policy_calls_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let removed = store.remove_policy("any").await.expect("remove ok");
        assert!(removed);
    }
}

impl PolicyStore {
    pub fn new(schema: Arc<Schema>, storage: Arc<dyn PolicyStorage>) -> Self {
        Self {
            storage,
            validator: Validator::new(schema.as_ref().clone()),
        }
    }

    pub async fn add_policy(&self, policy: Policy) -> Result<(), String> {
        // Build a PolicySet containing the single policy to validate
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy to set: {}", e))?;

        // Validate the policy set using Cedar's validator
        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            self.storage
                .save_policy(&policy)
                .await
                .map_err(|e| e.to_string())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Policy validation failed: {}", errors.join(", ")))
        }
    }

    pub async fn remove_policy(&self, id: &str) -> Result<bool, String> {
        self.storage
            .delete_policy(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_current_policy_set(&self) -> Result<PolicySet, StorageError> {
        let policies = self.storage.load_all_policies().await?;
        let mut policy_set = PolicySet::new();
        for policy in policies {
            policy_set
                .add(policy)
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
        }
        Ok(policy_set)
    }

    pub async fn get_policy(&self, id: &str) -> Result<Option<Policy>, String> {
        self.storage
            .get_policy_by_id(id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Update an existing policy by removing the old one and adding the new one
    pub async fn update_policy(&self, old_id: &str, new_policy: Policy) -> Result<(), String> {
        // Eliminar política antigua
        let removed = self.remove_policy(old_id).await?;
        if !removed {
            return Err(format!("Policy '{}' not found", old_id));
        }

        // Agregar nueva política (esto valida automáticamente)
        self.add_policy(new_policy).await
    }

    /// Validate a policy without persisting it
    pub fn validate_policy(&self, policy: &Policy) -> Result<(), String> {
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy: {}", e))?;

        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            Ok(())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Validation failed: {}", errors.join(", ")))
        }
    }
}
</file>

<file path="crates/policies/src/shared/domain/ports.rs">
use crate::shared::Hrn;
use async_trait::async_trait;
use cedar_policy::{EntityId, EntityTypeName, EntityUid, Policy, RestrictedExpression};
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("Policy parsing error: {0}")]
    ParsingError(String),
}

/// Attribute types to describe Cedar schema attributes in a typed way
#[derive(Debug, Clone)]
pub enum AttributeType {
    Primitive(&'static str), // e.g. "String", "Long", "Bool"
    Set(Box<AttributeType>), // e.g. Set<String>
    EntityId(&'static str),  // e.g. EntityId<Principal> (pass the entity type name)
}

impl AttributeType {
    pub fn to_cedar_decl(&self) -> String {
        match self {
            AttributeType::Primitive(name) => name.to_string(),
            AttributeType::Set(inner) => format!("Set<{}>", inner.to_cedar_decl()),
            AttributeType::EntityId(entity_ty) => format!("EntityId<{}>", entity_ty),
        }
    }
}

/// Type-level metadata for building Cedar schema fragments from Rust types
pub trait HodeiEntityType {
    /// Devuelve el nombre del 'servicio' que actúa como espacio de nombres.
    /// Ejemplo: "IAM", "Billing", "S3".
    fn service_name() -> &'static str;

    /// Devuelve el nombre local del tipo de recurso.
    /// Ejemplo: "User", "Group", "Bucket".
    fn resource_type_name() -> &'static str;

    /// **Método de conveniencia (con implementación por defecto).**
    /// Construye el `EntityTypeName` completo para Cedar a partir de las partes.
    fn cedar_entity_type_name() -> EntityTypeName {
        let namespace = Hrn::to_pascal_case(Self::service_name());
        let type_str = format!("{}::{}", namespace, Self::resource_type_name());
        EntityTypeName::from_str(&type_str)
            .expect("Failed to create EntityTypeName from service and resource type")
    }

    /// DEPRECATED: Use `cedar_entity_type_name()` instead.
    /// Mantener por compatibilidad temporal.
    fn entity_type_name() -> &'static str {
        // Fallback para compatibilidad: usa resource_type_name
        Self::resource_type_name()
    }

    /// Whether this entity type is a Principal in Cedar terms
    fn is_principal_type() -> bool {
        false
    }

    /// Optional: declare attributes in a typed fashion
    /// Default: empty, but recommended to provide for typed schema generation
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        Vec::new()
    }

    /// Optional: declare conceptual parent types (for membership semantics)
    /// Default: empty; membership will be modeled at data level via parents()
    fn cedar_parents_types() -> Vec<&'static str> {
        Vec::new()
    }
}

pub trait HodeiEntity {
    fn hrn(&self) -> &Hrn;
    fn attributes(&self) -> std::collections::HashMap<String, RestrictedExpression>;
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
    fn euid(&self) -> EntityUid {
        let hrn = self.hrn();
        let eid = EntityId::from_str(hrn.resource_id.as_str()).unwrap();
        let type_name: EntityTypeName =
            EntityTypeName::from_str(hrn.resource_type.as_str()).unwrap();
        EntityUid::from_type_name_and_id(type_name, eid)
    }
}

///A marker trait for entities that can act as 'principals'.
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// A marker trait for entities that can act as 'resources'.
pub trait Resource: HodeiEntity + HodeiEntityType {}

/// Define an action that can be registered in thepolicy engine.
pub trait Action {
    /// The unique identifier of the action.
    fn name() -> &'static str;

    /// Define which types of Principal and Resource this action applies to.
    /// This will be used to generate the Cedar schema.
    fn applies_to() -> (EntityTypeName, EntityTypeName);
}

#[async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError>;
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError>;
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/embedded_storage.rs">
// Feature gate is already applied at the module level in mod.rs

use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

#[derive(Clone)]
pub struct SurrealEmbeddedStorage {
    db: Surreal<Db>,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealEmbeddedStorage {
    /// path: filesystem path for RocksDB directory
    pub async fn new(namespace: &str, database: &str, path: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<RocksDb>(path)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            table: "policies".into(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealEmbeddedStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord {
                src: policy.to_string(),
            })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let res: Option<PolicyRecord> = self
            .db
            .delete((self.table.as_str(), id))
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
    }

    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError> {
        let thing = (self.table.as_str(), id);
        let rec: Option<PolicyRecord> = self
            .db
            .select(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;

        match rec {
            Some(r) => {
                let policy = r
                    .src
                    .parse::<Policy>()
                    .map_err(|e| StorageError::ParsingError(e.to_string()))?;
                Ok(Some(policy))
            }
            None => Ok(None),
        }
    }

    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        let recs: Vec<PolicyRecord> = self
            .db
            .select(self.table.as_str())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        let mut out = Vec::with_capacity(recs.len());
        for r in recs {
            let p = r
                .src
                .parse::<Policy>()
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
            out.push(p);
        }
        Ok(out)
    }
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/mem_storage.rs">
use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

#[derive(Clone)]
pub struct SurrealMemStorage {
    db: Surreal<Db>,
    _namespace: String,
    _database: String,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealMemStorage {
    pub async fn new(namespace: &str, database: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            _namespace: namespace.to_string(),
            _database: database.to_string(),
            table: "policies".to_string(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealMemStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord {
                src: policy.to_string(),
            })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let thing = (self.table.as_str(), id);
        let res: Option<PolicyRecord> = self
            .db
            .delete(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
    }

    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError> {
        let thing = (self.table.as_str(), id);
        let rec: Option<PolicyRecord> = self
            .db
            .select(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;

        match rec {
            Some(r) => {
                let policy = r
                    .src
                    .parse::<Policy>()
                    .map_err(|e| StorageError::ParsingError(e.to_string()))?;
                Ok(Some(policy))
            }
            None => Ok(None),
        }
    }

    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        let recs: Vec<PolicyRecord> = self
            .db
            .select(self.table.as_str())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        let mut out = Vec::with_capacity(recs.len());
        for r in recs {
            let p = r
                .src
                .parse::<Policy>()
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
            out.push(p);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn can_save_and_load_policy() {
        let storage = SurrealMemStorage::new("test_ns", "test_db")
            .await
            .expect("connect mem surreal");
        let src = r#"permit(principal, action, resource);"#;
        let p: Policy = src.parse().expect("parse policy");
        storage.save_policy(&p).await.expect("save");
        let all = storage.load_all_policies().await.expect("load");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].to_string(), p.to_string());
        let removed = storage
            .delete_policy(&p.id().to_string())
            .await
            .expect("delete");
        assert!(removed);
    }
}
</file>

<file path="crates/policies/tests/test_schema.rs">
//! Test to verify schema implementation works correctly

use cedar_policy::{
    Context, Entities, Entity, EntityUid, PolicySet, Request, RestrictedExpression, Schema,
    SchemaFragment,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

// Define a simple test entity that implements HodeiEntity
#[derive(Debug, Clone)]
struct TestUser {
    id: String,
    name: String,
    email: String,
}

impl TestUser {
    fn new(id: String, name: String, email: String) -> Self {
        Self { id, name, email }
    }

    fn euid(&self) -> EntityUid {
        EntityUid::from_str(&format!("User::\"{}\"", self.id)).unwrap()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

#[test]
fn test_complete_schema_build() {
    // Test the complete schema
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let result = SchemaFragment::from_cedarschema_str(schema_str);
    assert!(
        result.is_ok(),
        "Failed to create schema fragment: {:?}",
        result.err()
    );

    let (fragment, warnings) = result.unwrap();
    for warning in warnings {
        println!("Warning: {}", warning);
    }

    // Try to build a complete schema
    let schema_result = Schema::from_schema_fragments([fragment]);
    assert!(
        schema_result.is_ok(),
        "Failed to build complete schema: {:?}",
        schema_result.err()
    );

    let schema = schema_result.unwrap();

    // Try to create a validator
    let _validator = cedar_policy::Validator::new(schema.clone());

    // Test creating an entity with RestrictedExpression attributes
    let user = TestUser::new(
        "test_user".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    );
    let parents: HashSet<_> = user.parents().into_iter().collect();
    let entity = Entity::new(user.euid(), user.attributes(), parents);
    assert!(
        entity.is_ok(),
        "Failed to create entity: {:?}",
        entity.err()
    );
}

#[test]
fn test_policy_evaluation_with_restricted_expressions() -> Result<(), Box<dyn std::error::Error>> {
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
    let schema = Schema::from_schema_fragments([schema_fragment])?;

    // Create a simple policy
    let policy_str = r#"permit(
        principal == User::"alice",
        action == Action::"access",
        resource == User::"bob"
    );"#;

    let policy = policy_str.parse()?;

    // Create entities
    let alice_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Alice".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("alice@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let alice_entity = Entity::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        alice_attrs,
        HashSet::new(),
    )?;

    let bob_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Bob".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("bob@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let bob_entity = Entity::new(
        EntityUid::from_str(r#"User::"bob""#)?,
        bob_attrs,
        HashSet::new(),
    )?;

    let entities = Entities::from_entities(vec![alice_entity, bob_entity], None).expect("entities");
    let policies = PolicySet::from_policies([policy])?;

    let request = Request::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        EntityUid::from_str(r#"Action::"access""#)?,
        EntityUid::from_str(r#"User::"bob""#)?,
        Context::empty(),
        Some(&schema),
    )?;

    let authorizer = cedar_policy::Authorizer::new();
    let response = authorizer.is_authorized(&request, &policies, &entities);

    assert_eq!(response.decision(), cedar_policy::Decision::Allow);
    Ok(())
}
</file>

<file path="crates/policies/src/features/create_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct CreatePolicyCommand {
    pub policy_src: String,
}

impl CreatePolicyCommand {
    pub fn new(policy_src: impl Into<String>) -> Self {
        Self {
            policy_src: policy_src.into(),
        }
    }

    pub fn validate(&self) -> Result<(), CreatePolicyValidationError> {
        if self.policy_src.trim().is_empty() {
            return Err(CreatePolicyValidationError::EmptyPolicySource);
        }
        // Additional syntactic checks can be added here if needed
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CreatePolicyValidationError {
    #[error("policy source cannot be empty")]
    EmptyPolicySource,
}
</file>

<file path="crates/policies/src/features/create_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/policy_playground/di.rs">
use super::use_case::PolicyPlaygroundUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build PolicyPlaygroundUseCase (no storage required) and an AuthorizationEngine for consistency
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_policy_playground_use_case_mem() -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
    let (engine, _store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = PolicyPlaygroundUseCase::new();
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_policy_playground_use_case_embedded(
        path: &str,
    ) -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
        let (engine, _store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = PolicyPlaygroundUseCase::new();
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/validate_policy/di.rs">
use super::use_case::ValidatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build ValidatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_validate_policy_use_case_mem() -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ValidatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build ValidatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_validate_policy_use_case_embedded(
        path: &str,
    ) -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = ValidatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/validate_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::{ValidatePolicyQuery, ValidationError, ValidationResult};
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
}

pub struct ValidatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl ValidatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ValidatePolicyQuery,
    ) -> Result<ValidationResult, ValidatePolicyError> {
        // 1. Validar query
        query
            .validate()
            .map_err(|e| ValidatePolicyError::InvalidQuery(e.to_string()))?;

        // 2. Intentar parsear la política
        let policy_result: Result<Policy, _> = query.policy_content.parse();

        match policy_result {
            Ok(policy) => {
                // 3. Validar contra el schema usando el validator del store
                match self.store.validate_policy(&policy) {
                    Ok(()) => Ok(ValidationResult {
                        is_valid: true,
                        errors: vec![],
                        warnings: vec![],
                    }),
                    Err(e) => Ok(ValidationResult {
                        is_valid: false,
                        errors: vec![ValidationError {
                            message: e,
                            line: None,
                            column: None,
                        }],
                        warnings: vec![],
                    }),
                }
            }
            Err(e) => Ok(ValidationResult {
                is_valid: false,
                errors: vec![ValidationError {
                    message: format!("Parse error: {}", e),
                    line: None,
                    column: None,
                }],
                warnings: vec![],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn validate_policy_accepts_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("permit(principal, action, resource);".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }

    #[tokio::test]
    async fn validate_policy_rejects_invalid_syntax() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query =
            ValidatePolicyQuery::new("this is not valid cedar syntax".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(!result.is_valid);
        assert!(result.errors.len() > 0);
        assert!(result.errors[0].message.contains("Parse error"));
    }

    #[tokio::test]
    async fn validate_policy_rejects_empty_content() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("".to_string());
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(ValidatePolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }

    #[tokio::test]
    async fn validate_policy_accepts_complex_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let complex_policy = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let query = ValidatePolicyQuery::new(complex_policy.to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }
}
</file>

<file path="crates/policies/src/shared/application/mod.rs">
// application layer
mod engine;
mod store;
pub mod parallel;
pub mod di_helpers;

pub use engine::{AuthorizationEngine, AuthorizationRequest, EngineBuilder};
pub use store::PolicyStore;
</file>

<file path="crates/policies/src/shared/domain/hrn.rs">
use cedar_policy::{EntityId, EntityTypeName, EntityUid};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    pub account_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl Hrn {
    /// Convención AWS: nombre de servicio siempre en minúsculas (puede contener dígitos y '-')
    pub fn normalize_service_name(service: &str) -> String {
        service.to_ascii_lowercase()
    }

    /// Convierte 'iam' o 'my-service' a 'Iam' o 'MyService' (namespace Cedar)
    pub fn to_pascal_case(s: &str) -> String {
        s.split(|c| c == '-' || c == '_')
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    Some(f) => f.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn new(
        partition: String,
        service: String,
        account_id: String,
        resource_type: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(&service),
            account_id,
            resource_type,
            resource_id,
        }
    }

    /// Constructor usando HodeiEntityType para garantizar consistencia
    ///
    /// Este método construye un HRN usando la información del tipo, eliminando
    /// la posibilidad de desincronización entre el esquema y las instancias.
    ///
    /// # Ejemplo
    /// ```ignore
    /// use policies::shared::domain::hrn::Hrn;
    /// use hodei_iam::User; // From hodei-iam crate
    ///
    /// let user_hrn = Hrn::for_entity_type::<User>(
    ///     "hodei".to_string(),
    ///     "default".to_string(),
    ///     "user-123".to_string(),
    /// );
    /// ```
    pub fn for_entity_type<T: crate::shared::domain::ports::HodeiEntityType>(
        partition: String,
        account_id: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(T::service_name()),
            account_id,
            resource_type: T::resource_type_name().to_string(),
            resource_id,
        }
    }

    pub fn from_string(hrn_str: &str) -> Option<Self> {
        let parts: Vec<&str> = hrn_str.split(':').collect();
        if parts.len() != 6 || parts[0] != "hrn" {
            return None;
        }

        let resource_parts: Vec<&str> = parts[5].splitn(2, '/').collect();
        if resource_parts.len() != 2 {
            return None;
        }

        Some(Hrn {
            partition: parts[1].to_string(),
            service: Self::normalize_service_name(parts[2]),
            account_id: parts[4].to_string(), // El 3er segmento (region) se omite
            resource_type: resource_parts[0].to_string(),
            resource_id: resource_parts[1].to_string(),
        })
    }

    /// Convert HRN to Cedar EntityUid con namespace PascalCase (p.ej., Iam::User)
    ///
    /// Cedar expects UIDs as `Type::"id"`, where Type may be namespaced like `App::User`.
    /// We map:
    /// - Type: if `resource_type` already contains `::`, it's used as-is.
    ///   otherwise, when `service` is non-empty we construct `"{service}::{resource_type}"`.
    ///   both components are normalized to valid Cedar identifiers.
    /// - Id: always quoted string; if parsing fails, we wrap in quotes.
    pub fn euid(&self) -> EntityUid {
        // Namespace Cedar con PascalCase derivado del servicio
        let namespace = Self::to_pascal_case(&self.service);
        let type_str = if self.resource_type.contains("::") {
            self.resource_type.clone()
        } else if !namespace.is_empty() {
            format!("{}::{}", namespace, Self::normalize_ident(&self.resource_type))
        } else {
            Self::normalize_ident(&self.resource_type)
        };

        let eid = EntityId::from_str(&self.resource_id)
            .or_else(|_| EntityId::from_str(&format!("\"{}\"", self.resource_id)))
            .expect("Failed to create EntityId");
        let type_name =
            EntityTypeName::from_str(&type_str).expect("Failed to create EntityTypeName");
        EntityUid::from_type_name_and_id(type_name, eid)
    }

    /// Normalize a free-form string into a Cedar identifier segment
    /// - first char must be [A-Za-z_]; others may include digits
    /// - non-conforming chars are replaced by '_'
    fn normalize_ident(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        if let Some(c0) = chars.next() {
            let c = if c0.is_ascii_alphabetic() || c0 == '_' { c0 } else { '_' };
            out.push(c);
        } else {
            out.push('_');
        }
        for c in chars {
            if c.is_ascii_alphanumeric() || c == '_' { out.push(c); } else { out.push('_'); }
        }
        out
    }

    /// Convenience constructor for Action identifiers. This creates an HRN that
    /// translates into an EntityUid of the form `<service>::Action::"name"` when
    /// `service` is provided, otherwise `Action::"name"`.
    pub fn action(service: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            partition: "aws".to_string(),
            service: Self::normalize_service_name(&service.into()),
            account_id: String::new(),
            resource_type: "Action".to_string(),
            resource_id: name.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_hrn_roundtrip() {
        let s = "hrn:aws:hodei::123456789012:User/alice";
        let hrn = Hrn::from_string(s).expect("parse hrn");
        assert_eq!(hrn.partition, "aws");
        assert_eq!(hrn.service, "hodei");
        assert_eq!(hrn.account_id, "123456789012");
        assert_eq!(hrn.resource_type, "User");
        assert_eq!(hrn.resource_id, "alice");
        let rendered = hrn.to_string();
        assert!(rendered.contains("User/alice"));
    }

    #[test]
    fn euid_is_constructed() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        // Basic sanity: formatting should include type and id
        let s = format!("{}", euid);
        assert!(s.contains("User"));
        assert!(s.contains("alice"));
    }

    #[test]
    fn euid_uses_service_namespace_for_type() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei-svc".to_string(),
            "123".to_string(),
            "User-Profile".to_string(),
            "bob".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        // Expect PascalCase namespace and normalized type (guiones convertidos a guiones bajos)
        assert!(s.contains("HodeiSvc::User_Profile"));
        assert!(s.contains("\"bob\""));
    }

    #[test]
    fn euid_uses_pascal_namespace() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        assert!(s.contains("Iam::User"));
        assert!(s.contains("\"alice\""));
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hrn:{}:{}::{}:{}/{}",
            self.partition, self.service, self.account_id, self.resource_type, self.resource_id
        )
    }
}
</file>

<file path="crates/policies/src/lib.rs">
// crates/policies/src/lib.rs

pub mod shared;
pub use shared as domain;
// backward-compatible alias
pub mod features;
</file>

<file path="crates/policies/tests/domain_compilation_test.rs">
//! Test to verify that the domain modules compile correctly

#[cfg(test)]
mod tests {
    use policies::domain::HodeiEntityType;
    use policies::shared::application::EngineBuilder;

    // Tipos de prueba locales que representan entidades del dominio (ahora en IAM)
    struct TestUserType;
    struct TestGroupType;

    impl HodeiEntityType for TestUserType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "User" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![
                ("name", policies::domain::AttributeType::Primitive("String")),
                ("email", policies::domain::AttributeType::Primitive("String")),
            ]
        }
    }

    impl HodeiEntityType for TestGroupType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "Group" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![ ("name", policies::domain::AttributeType::Primitive("String")) ]
        }
    }

    #[test]
    fn test_user_entity_type() {
        assert_eq!(TestUserType::entity_type_name(), "User");
        // cedar_entity_type_name debe incluir el namespace en PascalCase
        let ty = TestUserType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::User");
    }

    #[test]
    fn test_group_entity_type() {
        assert_eq!(TestGroupType::entity_type_name(), "Group");
        let ty = TestGroupType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::Group");
    }

    #[test]
    fn test_user_cedar_attributes_present() {
        let attrs = TestUserType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestUserType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_group_cedar_attributes_present() {
        let attrs = TestGroupType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestGroupType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_engine_builder() {
        let _builder = EngineBuilder::new();
        // Just testing that we can create an engine builder
        assert!(true);
    }
}
</file>

<file path="crates/policies/src/features/delete_policy/use_case.rs">
use std::sync::Arc;

use super::dto::DeletePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum DeletePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct DeletePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl DeletePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &DeletePolicyCommand) -> Result<bool, DeletePolicyError> {
        // Validate command
        cmd.validate()
            .map_err(|e| DeletePolicyError::InvalidCommand(e.to_string()))?;

        // Delete policy from store
        let deleted = self
            .store
            .remove_policy(&cmd.policy_id)
            .await
            .map_err(DeletePolicyError::Storage)?;

        // Return error if policy was not found
        if !deleted {
            return Err(DeletePolicyError::NotFound(cmd.policy_id.clone()));
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;
    use cedar_policy::Policy;

    #[tokio::test]
    async fn delete_policy_removes_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Verify it exists
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());

        // Now delete it
        let uc = DeletePolicyUseCase::new(store.clone());
        let cmd = DeletePolicyCommand::new(policy_id.clone());
        let result = uc.execute(&cmd).await.expect("delete policy");

        assert!(result);

        // Verify it's gone
        let retrieved_after = store
            .get_policy(&policy_id)
            .await
            .expect("get policy after delete");
        assert!(retrieved_after.is_none());
    }

    #[tokio::test]
    async fn delete_policy_returns_not_found_for_nonexistent_policy() {
        // Build engine/store with real mem storage and schema
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_whitespace_only_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("   ");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }
}
</file>

<file path="crates/policies/src/features/get_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::GetPolicyQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct GetPolicyUseCase {
    store: Arc<PolicyStore>,
}

impl GetPolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, query: &GetPolicyQuery) -> Result<Policy, GetPolicyError> {
        // Validate query
        query
            .validate()
            .map_err(|e| GetPolicyError::InvalidQuery(e.to_string()))?;

        // Get policy from store
        let policy = self
            .store
            .get_policy(&query.policy_id)
            .await
            .map_err(GetPolicyError::Storage)?;

        // Return policy or error if not found
        policy.ok_or_else(|| GetPolicyError::NotFound(query.policy_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn get_policy_returns_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Now get it
        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new(policy_id.clone());
        let retrieved = uc.execute(&query).await.expect("get policy");

        assert_eq!(retrieved.id().to_string(), policy_id);
        assert_eq!(retrieved.to_string(), policy.to_string());
    }

    #[tokio::test]
    async fn get_policy_returns_none_when_not_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("nonexistent_policy_id");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn get_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }
}
</file>

<file path="crates/policies/src/features/list_policies/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::ListPoliciesQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesError {
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct ListPoliciesUseCase {
    store: Arc<PolicyStore>,
}

impl ListPoliciesUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ListPoliciesQuery,
    ) -> Result<Vec<Policy>, ListPoliciesError> {
        // Validate query
        query
            .validate()
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Get all policies from store
        let policy_set = self
            .store
            .get_current_policy_set()
            .await
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Convert PolicySet to Vec<Policy>
        let mut policies: Vec<Policy> = policy_set.policies().cloned().collect();

        // Apply filter if specified
        if let Some(filter_id) = &query.filter_id {
            policies.retain(|p| p.id().to_string().contains(filter_id));
        }

        // Apply pagination if specified
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(policies.len());

        // Skip offset items and take limit items
        let paginated_policies: Vec<Policy> =
            policies.into_iter().skip(offset).take(limit).collect();

        Ok(paginated_policies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn list_policies_returns_empty_when_no_policies() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_returns_single_policy_after_adding() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        // List policies - should have 1
        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1, "Expected 1 policy after adding one");
        assert_eq!(policies[0].id().to_string(), policy_id);
        assert_eq!(policies[0].to_string().trim(), policy.to_string().trim());
    }

    #[tokio::test]
    async fn list_policies_works_with_valid_cedar_policies() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy with conditions
        let conditional_policy_src = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let conditional_policy: Policy = conditional_policy_src
            .parse()
            .expect("parse conditional policy");
        store
            .add_policy(conditional_policy.clone())
            .await
            .expect("add conditional policy");

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1);
        assert_eq!(
            policies[0].id().to_string(),
            conditional_policy.id().to_string()
        );
    }

    #[tokio::test]
    async fn list_policies_with_pagination() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with limit
        let query = ListPoliciesQuery::with_pagination(0, 10);
        let policies = uc.execute(&query).await.expect("list policies");
        assert_eq!(policies.len(), 1);

        // Test with offset that skips all
        let query_skip = ListPoliciesQuery::with_pagination(1, 10);
        let policies_skip = uc
            .execute(&query_skip)
            .await
            .expect("list policies with skip");
        assert_eq!(policies_skip.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_validates_limit() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ListPoliciesUseCase::new(store);

        // Test with invalid limit (0)
        let query_zero = ListPoliciesQuery::with_pagination(0, 0);
        let result_zero = uc.execute(&query_zero).await;
        assert!(result_zero.is_err());

        // Test with invalid limit (> 1000)
        let query_large = ListPoliciesQuery::with_pagination(0, 1001);
        let result_large = uc.execute(&query_large).await;
        assert!(result_large.is_err());
    }

    #[tokio::test]
    async fn list_policies_with_filter() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with matching filter
        let query_match = ListPoliciesQuery::with_filter(policy_id.clone());
        let policies_match = uc
            .execute(&query_match)
            .await
            .expect("list policies with filter");
        assert_eq!(policies_match.len(), 1);

        // Test with non-matching filter
        let query_no_match = ListPoliciesQuery::with_filter("nonexistent".to_string());
        let policies_no_match = uc
            .execute(&query_no_match)
            .await
            .expect("list policies with no match");
        assert_eq!(policies_no_match.len(), 0);
    }
}
</file>

<file path="crates/policies/src/features/mod.rs">
// Las features se implementarán según se necesiten
// Por ahora, este módulo está vacío y listo para agregar features
pub mod create_policy;
pub mod get_policy;
pub mod list_policies;
pub mod delete_policy;
pub mod update_policy;
pub mod validate_policy;
pub mod policy_playground;
pub mod policy_playground_traces;
pub mod policy_analysis;
pub mod batch_eval;
</file>

<file path="crates/policies/src/shared/domain/mod.rs">
// Local modules in shared/domain
pub mod entity_utils;
pub mod error;
pub mod hrn;
pub mod ports;
pub mod schema_assembler;

// Convenience re-exports for external use
pub use error::HodeiPoliciesError;
pub use hrn::Hrn;
pub use ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError};
</file>

<file path="crates/policies/src/shared/domain/schema_assembler.rs">
//! Typed schema assembler: builds Cedar schema fragments from HodeiEntityType metadata

use crate::shared::HodeiEntityType;
use cedar_policy::{CedarSchemaError, SchemaFragment};
use std::fmt::Write as _;

fn is_lowercase(s: &str) -> bool { s.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase()) }
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() { Some(c0) if c0.is_ascii_uppercase() => {}, _ => return false }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn invalid_schema_error() -> Box<CedarSchemaError> {
    // Genera un SchemaError intentando parsear un esquema inválido
    let invalid_schema = "entity Invalid { invalid_attr: InvalidType };";
    match SchemaFragment::from_cedarschema_str(invalid_schema) {
        Err(e) => Box::new(e),
        Ok(_) => {
            // Si por alguna razón el esquema inválido es válido, intentamos con otro
            let conflicting = r#"
                entity Test {};
                entity Test {};
            "#;
            match SchemaFragment::from_cedarschema_str(conflicting) {
                Err(e) => Box::new(e),
                Ok(_) => panic!("Failed to generate a SchemaError"),
            }
        }
    }
}

/// Generate a Cedar SchemaFragment for a given entity type `T`.
///
/// Uses the new service_name() and resource_type_name() methods to construct
/// the fully qualified entity type name (e.g., "IAM::User").
pub fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<CedarSchemaError>>
{
    // Validación de convenciones
    let service = T::service_name();
    let resource = T::resource_type_name();
    if !is_lowercase(service) { return Err(invalid_schema_error()); }
    if !is_pascal_case(resource) { return Err(invalid_schema_error()); }

    let attrs = T::cedar_attributes();

    let mut s = String::new();
    
    // Para entidades con namespace, necesitamos declarar el namespace primero
    let namespace = crate::shared::Hrn::to_pascal_case(service);
    let _ = writeln!(s, "namespace {} {{", namespace);
    
    // No usamos "in [Principal]" porque Principal debe estar definido globalmente
    // En su lugar, las entidades principales se identifican por su uso en las acciones

    // entity Header (sin el namespace, ya que estamos dentro del bloque namespace)
    let _ = writeln!(s, "    entity {} {{", resource);

    for (i, (name, atype)) in attrs.iter().enumerate() {
        if i < attrs.len() - 1 {
            let _ = writeln!(s, "        {}: {},", name, atype.to_cedar_decl());
        } else {
            let _ = writeln!(s, "        {}: {}", name, atype.to_cedar_decl());
        }
    }
    // Close entity
    let _ = writeln!(s, "    }};");
    // Close namespace
    let _ = writeln!(s, "}}");

    // Build fragment
    let (frag, _warnings) =
        SchemaFragment::from_cedarschema_str(&s).expect("typed fragment generation should parse");
    Ok(frag)
}
</file>

<file path="crates/policies/src/shared/mod.rs">
// Facade raíz del crate policies (estructura hexagonal interna)
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-exports para tests e integración
pub use application::{AuthorizationEngine, AuthorizationRequest, EngineBuilder, PolicyStore};
pub use domain::{
    entity_utils,
    hrn::Hrn,
    ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError},
    schema_assembler::*,
};

// Re-exports de Cedar comunes en tests
pub use cedar_policy::{Context, EntityUid, Policy, PolicyId};
</file>

<file path="crates/policies/tests/principals_schema_test.rs">
use async_trait::async_trait;
use cedar_policy::{EntityUid, RestrictedExpression};
use policies::shared::application::EngineBuilder;
use policies::shared::domain::ports::{
    AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError,
};
use policies::shared::Hrn;
use std::collections::HashMap;
use std::sync::Arc;

struct DummyStorage;

#[async_trait]
impl PolicyStorage for DummyStorage {
    async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(
        &self,
        _id: &str,
    ) -> Result<Option<cedar_policy::Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
        Ok(vec![])
    }
}

// Tipos de prueba locales (sustituyen a principals::{User, Group} que ahora viven en IAM)
struct TestUser {
    hrn: Hrn,
}

struct TestGroup {
    hrn: Hrn,
}

// Implementación de HodeiEntityType para TestUser
impl HodeiEntityType for TestUser {
    fn service_name() -> &'static str {
        "iam"  // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("email", AttributeType::Primitive("String")),
        ]
    }
    fn is_principal_type() -> bool {
        true
    }
}

// Implementación de HodeiEntity para TestUser
impl HodeiEntity for TestUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Principal para TestUser
impl Principal for TestUser {}

// Implementación de HodeiEntityType para TestGroup
impl HodeiEntityType for TestGroup {
    fn service_name() -> &'static str {
        "iam"  // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![("name", AttributeType::Primitive("String"))]
    }
}

// Implementación de HodeiEntity para TestGroup
impl HodeiEntity for TestGroup {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Resource para TestGroup
impl Resource for TestGroup {}

#[tokio::test]
async fn engine_builder_registers_dummy_entities_and_builds() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<TestUser>()
        .expect("register TestUser")
        .register_resource::<TestGroup>()
        .expect("register TestGroup");

    let res = builder.build(storage);
    assert!(res.is_ok(), "engine build should succeed: {:?}", res.err());
}
</file>

<file path="crates/policies/src/features/create_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::CreatePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum CreatePolicyError {
    #[error("invalid_policy: {0}")]
    InvalidPolicy(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct CreatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl CreatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &CreatePolicyCommand) -> Result<(), CreatePolicyError> {
        // Validate command
        cmd.validate()
            .map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;

        let policy: Policy = cmd
            .policy_src
            .parse::<Policy>()
            .map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;
        self.store
            .add_policy(policy)
            .await
            .map_err(CreatePolicyError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn create_policy_persists_in_surreal_mem() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = CreatePolicyUseCase::new(store);
        let cmd = crate::features::create_policy::dto::CreatePolicyCommand::new(
            r#"permit(principal, action, resource);"#,
        );
        uc.execute(&cmd).await.expect("create policy");

        // Ensure it's in the current set
        let pset = engine
            .store
            .get_current_policy_set()
            .await
            .expect("policy set");
        assert!(pset.to_cedar().is_some());
    }
}
</file>

<file path="crates/policies/src/features/delete_policy/di.rs">
use super::use_case::DeletePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build DeletePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
pub async fn make_delete_policy_use_case_mem() -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = DeletePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build DeletePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
    pub async fn make_delete_policy_use_case_embedded(
        path: &str,
    ) -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = DeletePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/get_policy/di.rs">
use super::use_case::GetPolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build GetPolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_get_policy_use_case_mem() -> Result<(GetPolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = GetPolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build GetPolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_get_policy_use_case_embedded(
        path: &str,
    ) -> Result<(GetPolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = GetPolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/list_policies/di.rs">
use super::use_case::ListPoliciesUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build ListPoliciesUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_list_policies_use_case_mem() -> Result<(ListPoliciesUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ListPoliciesUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build ListPoliciesUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_list_policies_use_case_embedded(
        path: &str,
    ) -> Result<(ListPoliciesUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = ListPoliciesUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/update_policy/di.rs">
use super::use_case::UpdatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build UpdatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_update_policy_use_case_mem() -> Result<(UpdatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = UpdatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build UpdatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_update_policy_use_case_embedded(
        path: &str,
    ) -> Result<(UpdatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = UpdatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/update_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::UpdatePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("policy_parse_error: {0}")]
    ParseError(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct UpdatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl UpdatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        cmd: &UpdatePolicyCommand,
    ) -> Result<Policy, UpdatePolicyError> {
        // 1. Validar comando
        cmd.validate()
            .map_err(|e| UpdatePolicyError::InvalidCommand(e.to_string()))?;

        // 2. Verificar que la política existe
        let existing = self
            .store
            .get_policy(&cmd.policy_id)
            .await
            .map_err(UpdatePolicyError::Storage)?;

        if existing.is_none() {
            return Err(UpdatePolicyError::NotFound(cmd.policy_id.clone()));
        }

        // 3. Parsear nueva política
        let new_policy: Policy = cmd
            .new_policy_content
            .parse()
            .map_err(|e| UpdatePolicyError::ParseError(format!("{}", e)))?;

        // 4. Actualizar política (esto valida automáticamente)
        self.store
            .update_policy(&cmd.policy_id, new_policy.clone())
            .await
            .map_err(UpdatePolicyError::ValidationError)?;

        Ok(new_policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn update_policy_successfully_updates_existing_policy() {
        // Arrange: Create engine/store and add a policy (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        // Act: Update the policy
        let uc = UpdatePolicyUseCase::new(store.clone());
        let new_content = r#"forbid(principal, action, resource);"#;
        let cmd = UpdatePolicyCommand::new(policy_id.clone(), new_content.to_string());
        let result = uc.execute(&cmd).await;

        // Assert: Should succeed
        assert!(result.is_ok());
        let updated_policy = result.unwrap();
        assert_eq!(updated_policy.to_string().trim(), new_content.trim());

        // Verify the policy was actually updated in storage
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().to_string().trim(), new_content.trim());
    }

    #[tokio::test]
    async fn update_policy_returns_not_found_for_nonexistent_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "nonexistent_policy_id".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_content() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new("policy_id".to_string(), "".to_string());
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_new_policy_syntax() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            policy_id,
            "this is not valid cedar syntax".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::ParseError(_)) => {}
            _ => panic!("Expected ParseError"),
        }
    }
}
</file>

<file path="crates/policies/src/shared/application/engine.rs">
use crate::shared::application::PolicyStore;
use crate::shared::domain::ports::{Action, Principal, Resource};
use crate::shared::domain::{HodeiEntity, PolicyStorage};
use crate::shared::generate_fragment_for_type;
use cedar_policy::{CedarSchemaError, Context, Entities, PolicySet, Request, Response, Schema, SchemaError, SchemaFragment};
use std::collections::HashSet;
use std::sync::Arc;

pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: cedar_policy::EntityUid,
    pub resource: &'a dyn HodeiEntity,
    pub context: Context,
    pub entities: Vec<&'a dyn HodeiEntity>,
}

#[derive(Clone)]
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response {
        let entity_vec: Vec<cedar_policy::Entity> = request
            .entities
            .iter()
            .map(|entity| {
                let attrs = entity.attributes();
                let parents: HashSet<_> = entity.parents().into_iter().collect();
                cedar_policy::Entity::new(entity.euid(), attrs, parents)
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to create entities");

        let entities = Entities::from_entities(entity_vec, None)
            .expect("Failed to create Entities collection");

        let cedar_request = Request::new(
            request.principal.euid(),
            request.action.clone(),
            request.resource.euid(),
            request.context.clone(),
            None,
        )
        .expect("Failed to create Cedar request");

        let policies = self
            .store
            .get_current_policy_set()
            .await
            .unwrap_or_else(|_| PolicySet::new());
        cedar_policy::Authorizer::new().is_authorized(&cedar_request, &policies, &entities)
    }
}

#[derive(Default)]
pub struct EngineBuilder {
    entity_fragments: Vec<SchemaFragment>,
    action_fragments: Vec<SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self { 
        Self::default() 
    }

    // New methods for the generic approach
    pub fn register_principal<P: Principal>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<P>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_resource<R: Resource>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<R>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_action<A: Action>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let (principal_type, resource_type) = A::applies_to();
        let schema_str = format!(
            "action \"{}\" appliesTo {{ principal: {}, resource: {} }};",
            A::name(), principal_type, resource_type
        );

        // Parse the action schema fragment
        let (frag, _warnings) = SchemaFragment::from_cedarschema_str(&schema_str)
            .map_err(|_e| {
                // If parsing fails, create a SchemaError by parsing an intentionally invalid schema
                // This ensures we return the correct error type
                let invalid = "entity Invalid { invalid: Invalid }";
                match SchemaFragment::from_cedarschema_str(invalid) {
                    Ok(_) => unreachable!(),
                    Err(_cedar_err) => {
                        // Create a generic schema parsing error using Schema::from_schema_fragments
                        // with an empty fragment list to trigger a schema error
                        Box::new(CedarSchemaError::from(
                            Schema::from_schema_fragments(vec![]).unwrap_err()
                        ))
                    }
                }
            })?;

        self.action_fragments.push(frag);
        Ok(self)
    }

    pub fn build(
        self,
        storage: Arc<dyn PolicyStorage>,
    ) -> Result<(AuthorizationEngine, PolicyStore), Box<SchemaError>> {
        // Build schema from registered fragments only
        // No automatic base schema - everything must be explicitly registered by the client
        let all_fragments = [self.entity_fragments, self.action_fragments].concat();
        
        let schema = Arc::new(Schema::from_schema_fragments(all_fragments)?);
        let store = PolicyStore::new(schema.clone(), storage);
        let engine = AuthorizationEngine { 
            schema, 
            store: store.clone() 
        };
        Ok((engine, store))
    }
}
</file>

<file path="crates/policies/Cargo.toml">
[package]
name = "policies"
version = "0.1.0"
edition = "2024"
license = "MIT"

[dependencies]
# Core dependencies
serde = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
shared = { path = "../shared" }
chrono = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }

# Cedar Policy Engine
cedar-policy = { workspace = true }

# Database - SurrealDB for policy storage (backend via crate features)
surrealdb = { workspace = true }

# Async runtime
tokio = { workspace = true, features = ["full"] }
async-trait = { workspace = true }

## testing dependencies are only declared under [dev-dependencies]

[features]
default = ["mem"]
mem = ["surrealdb/kv-mem"]
embedded = ["surrealdb/kv-rocksdb"]
integration = []

[dev-dependencies]
mockall = { workspace = true }
testcontainers = { workspace = true }
futures = { workspace = true }
uuid = { workspace = true }
regex = { workspace = true }
</file>

<file path="crates/policies/src/features/create_policy/di.rs">
use super::use_case::CreatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build CreatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered. 
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_create_policy_use_case_mem() -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = CreatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build CreatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_create_policy_use_case_embedded(
        path: &str,
    ) -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = CreatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

</files>
