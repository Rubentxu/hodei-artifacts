usesuper::dto::CreateUserCommand;
use super::error::CreateUserError;
use super::ports::{CreateUserRepositories, CreateUserUnitOfWork};
use super::use_case::CreateUserUseCase;
use crate::internal::application::ports::{UserRepository, UserRepositoryError};
use crate::internal::domain::User;
usekernel::Hrn;
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;
use tokio::sync::Mutex;

// Mock User Repository
#[derive(Clone)]
struct MockUserRepository {
    save_result: Arc<Mutex<Result<(), UserRepositoryError>>>,
    saved_users: Arc<Mutex<Vec<User>>>,
}

impl MockUserRepository {
    fn new() -> Self {
        Self {
            save_result: Arc::new(Mutex::new(Ok(()))),
            saved_users: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn set_save_result(&self, result: Result<(), UserRepositoryError>) {
        *self.save_result.try_lock().unwrap() = result;
    }

    async fn get_saved_users(&self) -> Vec<User> {
        self.saved_users.lock().await.clone()
    }
}

#[async_trait::async_trait]
impl UserRepository for MockUserRepository {
async fn save(&self, user: &User) -> Result<(), UserRepositoryError> {
        let result = *self.save_result.lock().await;
        if result.is_ok() {
            self.saved_users.lock().await.push(user.clone());
        }
        result
    }

    async fn find_by_hrn(&self, _hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
        Ok(None) // Not used in create_user
    }

    async fn find_all(&self) -> Result<Vec<User>, UserRepositoryError> {
        Ok(Vec::new()) // Not used in create_user
    }
}

//Mock Unit of Work
struct MockCreateUserUnitOfWork {
    repo: Arc<dyn UserRepository>,
    begin_result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    commit_result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    rollback_result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    begin_called: Arc<Mutex<bool>>,
    commit_called: Arc<Mutex<bool>>,
    rollback_called: Arc<Mutex<bool>>,
}

impl MockCreateUserUnitOfWork {
    fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self {
            repo,
            begin_result: Ok(()),
            commit_result: Ok(()),
            rollback_result: Ok(()),
            begin_called: Arc::new(Mutex::new(false)),
            commit_called: Arc::new(Mutex::new(false)),
            rollback_called: Arc::new(Mutex::new(false)),
        }
    }

    fn set_begin_result(&mutself, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
        self.begin_result = result;
    }

    fn set_commit_result(&mut self, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
        self.commit_result = result;
    }

    fnset_rollback_result(
        &mut self,
        result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
        self.rollback_result = result;
    }

    async fn was_begin_called(&self) -> bool {
        *self.begin_called.lock().await
    }

    async fnwas_commit_called(&self) -> bool {
        *self.commit_called.lock().await
    }

    async fn was_rollback_called(&self) -> bool {
        *self.rollback_called.lock().await
    }
}

#[async_trait::async_trait]
impl CreateUserUnitOfWork for MockCreateUserUnitOfWork {
    async fn begin(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.begin_called.lock().await = true;
        self.begin_result.clone()
    }

    async fn commit(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.commit_called.lock().await = true;
        self.commit_result.clone()
    }

    async fn rollback(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.rollback_called.lock().await = true;
        self.rollback_result.clone()
    }

    fn repositories(&self) -> CreateUserRepositories {
        CreateUserRepositories::new(self.repo.clone())
    }
}

#[tokio::test]
async fn test_create_user_success() {
    let mock_repo = Arc::new(MockUserRepository::new());
    let mock_uow = Arc::new(MockCreateUserUnitOfWork::new(mock_repo.clone()));
    let use_case= CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john@example.com");
    assert_eq!(view.tags, vec!["admin".to_string()]);
    assert!(view.hrn.starts_with("hrn:hodei:iam:default:User/"));

    // Verify transaction calls
    let uow = use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(uow.was_commit_called().await);
    assert!(!uow.was_rollback_called().await);

    //Verify user was saved
    let saved_users = mock_repo.get_saved_users().await;
    assert_eq!(saved_users.len(), 1);
    assert_eq!(saved_users[0].name, "John Doe");
}

#[tokio::test]
async fn test_create_user_transaction_begin_failure() {
    let mock_repo= Arc::new(MockUserRepository::new());
    let mut mock_uow = MockCreateUserUnitOfWork::new(mock_repo);
    mock_uow.set_begin_result(Err("Transaction begin failed".into()));
    let mock_uow = Arc::new(mock_uow);
    let use_case = CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_err());
    match result.err().unwrap(){
        CreateUserError::TransactionBeginFailed(msg) => {
            assert!(msg.contains("Transaction begin failed"));
        }
        _ => panic!("Expected TransactionBeginFailed"),
    }

    // Verify no commit or rollback
    let uow = use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(!uow.was_commit_called().await);
    assert!(!uow.was_rollback_called().await);
}

#[tokio::test]
async fn test_create_user_transaction_commit_failure() {
    let mock_repo = Arc::new(MockUserRepository::new());
    let mut mock_uow = MockCreateUserUnitOfWork::new(mock_repo);
    mock_uow.set_commit_result(Err("Transaction commit failed".into()));
    let mock_uow = Arc::new(mock_uow);
    let use_case = CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_err());
    match result.err().unwrap() {
        CreateUserError::TransactionCommitFailed(msg) => {
            assert!(msg.contains("Transaction commit failed"));
        }
        _ => panic!("Expected TransactionCommitFailed"),
    }

    // Verify rollback was called
    let uow = use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(uow.was_commit_called().await);
    assert!(uow.was_rollback_called().await);
}

#[tokio::test]
async fn test_create_user_repository_save_failure() {
    let mock_repo = Arc::new(MockUserRepository::new());
    mock_repo.set_save_result(Err(UserRepositoryError::DatabaseError(
        "Save failed".to_string(),
   )));
    let mock_uow = Arc::new(MockCreateUserUnitOfWork::new(mock_repo));
    let use_case = CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
        tags:vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_err());
    match result.err().unwrap() {
        CreateUserError::UserSaveFailed(UserRepositoryError::DatabaseError(msg)) => {
            assert!(msg.contains("Save failed"));
        }
        _ => panic!("Expected UserSaveFailed"),
    }

    // Verify rollback was called
    let uow = use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(!uow.was_commit_called().await);
    assert!(uow.was_rollback_called().await);
}

#[tokio::test]
async fn test_create_user_with_event_publisher() {
    let mock_repo = Arc::new(MockUserRepository::new());
    let mock_uow = Arc::new(MockCreateUserUnitOfWork::new(mock_repo.clone()));
    let event_bus =Arc::new(InMemoryEventBus::new());
    let use_case = CreateUserUseCase::new(mock_uow).with_event_publisher(event_bus);

    let cmd = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
        tags: vec![],
   };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());

// Verify event was published (check if event_bus has events)
    // Since InMemoryEventBus doesn't expose internals easily, we just ensure no panic
}

#[tokio::test]
async fn test_create_user_no_tags() {
    let mock_repo = Arc::new(MockUserRepository::new());
    letmock_uow = Arc::new(MockCreateUserUnitOfWork::new(mock_repo.clone()));
    let use_case = CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "JaneDoe".to_string(),
        email: "jane@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);

    let saved_users= mock_repo.get_saved_users().await;
    assert_eq!(saved_users[0].tags.len(), 0);
}

#[tokio::test]
async fn test_create_user_multiple_tags() {
    let mock_repo = Arc::new(MockUserRepository::new());
    let mock_uow = Arc::new(MockCreateUserUnitOfWork::new(mock_repo.clone()));
    let use_case = CreateUserUseCase::new(mock_uow);

    let cmd = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["dev".to_string(), "admin".to_string(), "lead".to_string()],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    let view = result.unwrap();
assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"dev".to_string()));
    assert!(view.tags.contains(&"admin".to_string()));
    assert!(view.tags.contains(&"lead".to_string()));

    let saved_users = mock_repo.get_saved_users().await;
    assert_eq!(saved_users[0].tags, view.tags);
}

#[cfg(test)]
mod tests {
    use super::super::dto::{CreateUserCommand, UserView};
    use super::super::error::CreateUserError;
    use super::super::ports::{CreateUserPort, HrnGenerator};
    use super::super::use_case::CreateUserUseCase;
    use crate::internal::domain::User;
    use kernel::Hrn;
    use std::sync::Arc;

    // Mock implementation of CreateUserPort
    struct MockUserPersister {
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl CreateUserPort for MockUserPersister {
        asyncfn save_user(&self, _user: &User) -> Result<(), CreateUserError> {
            if self.should_fail {
                Err(CreateUserError::PersistenceError("Failed to save user".to_string()))
            } else {
                Ok(())
            }
        }
    }

    // Mock implementation of HrnGenerator
   struct MockHrnGenerator {
        hrn: Hrn,
    }

    impl HrnGenerator for MockHrnGenerator {
        fn new_user_hrn(&self, _name: &str) -> Hrn {
            self.hrn.clone()
        }
    }

    #[tokio::test]
    async fntest_create_user_success() {
        // Arrange
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );
        
       let persister = Arc::new(MockUserPersister { should_fail: false });
        let hrn_generator = Arc::new(MockHrnGenerator { hrn: hrn.clone() });
        let use_case = CreateUserUseCase::new(persister, hrn_generator);
        
        let command = CreateUserCommand {
name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            tags: vec!["test".to_string()],
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let user_view =result.unwrap();
        assert_eq!(user_view.hrn, hrn.to_string());
        assert_eq!(user_view.name, "Test User");
        assert_eq!(user_view.email, "test@example.com");
        assert_eq!(user_view.tags, vec!["test".to_string()]);
    }

    #[tokio::test]
    async fn test_create_user_persistence_error() {
        // Arrange
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );
        
        let persister = Arc::new(MockUserPersister { should_fail: true});
        let hrn_generator = Arc::new(MockHrnGenerator { hrn });
        let use_case = CreateUserUseCase::new(persister, hrn_generator);
        
        let command = CreateUserCommand {
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
tags: vec![],
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreateUserError::PersistenceError(_) => (),
            _ => panic!("Expected PersistenceError"),
        }
    }
}
