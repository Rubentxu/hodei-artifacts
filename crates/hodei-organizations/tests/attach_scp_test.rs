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