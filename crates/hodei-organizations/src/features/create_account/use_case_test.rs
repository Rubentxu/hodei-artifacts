use crate::features::create_account::di::create_account_use_case;
use crate::features::create_account::dto::CreateAccountCommand;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::application::ports::account_repository::{
    AccountRepository, AccountRepositoryError,
};
use crate::shared::domain::account::Account;

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// In-memory implementation of AccountRepository used across tests to
/// ensure the AccountRepositoryAdapter (constructed via the DI function)
/// is always exercised (eliminating dead_code warnings for struct/new()).
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

#[async_trait]
impl AccountRepository for InMemoryAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let mut g = self.accounts.lock().unwrap();
        g.insert(account.hrn.to_string(), account.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let g = self.accounts.lock().unwrap();
        Ok(g.get(&hrn.to_string()).cloned())
    }
}

#[tokio::test]
async fn test_create_account_success() {
    // Arrange
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: Some(parent_hrn.clone()),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, Some(parent_hrn));
    assert!(!account_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_account_empty_name() {
    // Arrange
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "".to_string(),
        parent_hrn: Some(parent_hrn),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateAccountError::InvalidAccountName));
}

#[tokio::test]
async fn test_create_account_with_di() {
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-di".to_string(),
    );

    let command = CreateAccountCommand {
        name: "DIAccount".to_string(),
        parent_hrn: Some(parent_hrn.clone()),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "DIAccount");
    assert_eq!(view.parent_hrn, Some(parent_hrn));
}
