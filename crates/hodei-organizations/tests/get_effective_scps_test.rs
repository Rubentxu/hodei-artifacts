use hodei_organizations::shared::domain::ou::OrganizationalUnit;
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;
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