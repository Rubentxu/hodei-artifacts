hodei-artifacts/crates/hodei-iam/src/features/create_group/use_case_test.rs
use super::dto::CreateGroupCommand;
use super::error::CreateGroupError;
use super::ports::{CreateGroupRepositories, CreateGroupUnitOfWork};
use super::use_case::CreateGroupUseCase;
use crate::internal::application::ports::{GroupRepository, GroupRepositoryError};
use crate::internal::domain::Group;
use kernel::Hrn;
use kernel::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;
use tokio::sync::Mutex;

// Mock Group Repository
#[derive(Clone)]
struct MockGroupRepository {
    save_result: Arc<Mutex<Result<(), GroupRepositoryError>>>,
    saved_groups: Arc<Mutex<Vec<Group>>>,
}

impl MockGroupRepository {
    fn new() -> Self {
        Self {
            save_result: Arc::new(Mutex::new(Ok(()))),
           saved_groups: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn set_save_result(&self, result: Result<(), GroupRepositoryError>) {
        *self.save_result.try_lock().unwrap() = result;
    }

    async fn get_saved_groups(&self) -> Vec<Group> {
       self.saved_groups.lock().await.clone()
    }
}

#[async_trait::async_trait]
impl GroupRepository for MockGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), GroupRepositoryError> {
        let result = *self.save_result.lock().await;
        if result.is_ok() {
self.saved_groups.lock().await.push(group.clone());
        }
        result
    }

    async fn find_by_hrn(&self, _hrn: &Hrn) -> Result<Option<Group>, GroupRepositoryError> {
        Ok(None) // Not used in create_group
    }

    async fn find_all(&self) -> Result<Vec<Group>, GroupRepositoryError> {
        Ok(Vec::new()) // Not used in create_group
    }

    async fn find_by_name(&self, _name: &str) -> Result<Option<Group>, GroupRepositoryError> {
        Ok(None) // Not used in create_group
    }

async fn add_user(&self, _group_hrn: &Hrn, _user_hrn: &Hrn) -> Result<(), GroupRepositoryError> {
        Ok(()) // Not used in create_group
    }

    async fn remove_user(&self, _group_hrn: &Hrn, _user_hrn:&Hrn) -> Result<(), GroupRepositoryError> {
        Ok(()) // Not used in create_group
    }
}

// Mock Unit of Work
struct MockCreateGroupUnitOfWork {
    repo: Arc<dyn GroupRepository>,
    begin_result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    commit_result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    rollback_result: Result<(), Box<dynstd::error::Error + Send + Sync>>,
    begin_called: Arc<Mutex<bool>>,
    commit_called: Arc<Mutex<bool>>,
    rollback_called: Arc<Mutex<bool>>,
}

impl MockCreateGroupUnitOfWork {
    fn new(repo: Arc<dyn GroupRepository>) -> Self {
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

    fn set_begin_result(&mut self, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
        self.begin_result = result;
    }

    fn set_commit_result(&mut self, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
        self.commit_result = result;
}

    fn set_rollback_result(
        &mut self,
        result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) {
        self.rollback_result = result;
    }

    async fn was_begin_called(&self) -> bool {
        *self.begin_called.lock().await
   }

    async fn was_commit_called(&self) -> bool {
        *self.commit_called.lock().await
    }

    async fn was_rollback_called(&self) -> bool {
        *self.rollback_called.lock().await
    }
}

#[async_trait::async_trait]
impl CreateGroupUnitOfWork for MockCreateGroupUnitOfWork{
    async fn begin(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.begin_called.lock().await = true;
        self.begin_result.clone()
    }

    async fn commit(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>{
        *self.commit_called.lock().await = true;
        self.commit_result.clone()
    }

    async fn rollback(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        *self.rollback_called.lock().await = true;
        self.rollback_result.clone()
    }

    fn repositories(&self) -> CreateGroupRepositories {
        CreateGroupRepositories::new(self.repo.clone())
    }
}

#[tokio::test]
async fn test_create_group_success() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    let mock_uow = Arc::new(MockCreateGroupUnitOfWork::new(mock_repo.clone()));
    letuse_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    letview = result.unwrap();
    assert_eq!(view.name, "Admins");
    assert_eq!(view.tags, vec!["admin".to_string()]);
    assert!(view.hrn.starts_with("hrn:hodei:iam:default:Group/"));

    // Verify transaction calls
    let uow =use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(uow.was_commit_called().await);
    assert!(!uow.was_rollback_called().await);

    // Verify group was saved
    let saved_groups = mock_repo.get_saved_groups().await;
assert_eq!(saved_groups.len(), 1);
    assert_eq!(saved_groups[0].name, "Admins");
}

#[tokio::test]
async fn test_create_group_transaction_begin_failure() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    let mut mock_uow = MockCreateGroupUnitOfWork::new(mock_repo);
    mock_uow.set_begin_result(Err("Transaction begin failed".into()));
    let mock_uow = Arc::new(mock_uow);
    let use_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_err());
    match result.err().unwrap() {
        CreateGroupError::TransactionBeginFailed(msg) => {
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
async fn test_create_group_transaction_commit_failure() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    let mut mock_uow = MockCreateGroupUnitOfWork::new(mock_repo);
    mock_uow.set_commit_result(Err("Transaction commit failed".into()));
    let mock_uow = Arc::new(mock_uow);
    let use_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_err());
match result.err().unwrap() {
        CreateGroupError::TransactionCommitFailed(msg) => {
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
async fn test_create_group_repository_save_failure() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    mock_repo.set_save_result(Err(GroupRepositoryError::DatabaseError(
        "Save failed".to_string(),
   )));
    let mock_uow = Arc::new(MockCreateGroupUnitOfWork::new(mock_repo));
    let use_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Admins".to_string(),
        tags: vec![],
    };

    let result= use_case.execute(cmd).await;

    assert!(result.is_err());
    match result.err().unwrap() {
        CreateGroupError::GroupSaveFailed(GroupRepositoryError::DatabaseError(msg)) => {
            assert!(msg.contains("Save failed"));
        }
        _ => panic!("Expected GroupSaveFailed"),
    }

    // Verify rollback was called
    let uow = use_case.uow.as_ref();
    assert!(uow.was_begin_called().await);
    assert!(!uow.was_commit_called().await);
    assert!(uow.was_rollback_called().await);
}

#[tokio::test]
async fn test_create_group_with_event_publisher() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    let mock_uow = Arc::new(MockCreateGroupUnitOfWork::new(mock_repo.clone()));
    let event_bus =Arc::new(InMemoryEventBus::new());
    let use_case = CreateGroupUseCase::new(mock_uow).with_event_publisher(event_bus);

    let cmd = CreateGroupCommand {
        group_name: "Developers".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());

// Verify event was published (check if event_bus has events)
    // Since InMemoryEventBus doesn't expose internals easily, we just ensure no panic
}

#[tokio::test]
async fn test_create_group_no_tags() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    letmock_uow = Arc::new(MockCreateGroupUnitOfWork::new(mock_repo.clone()));
    let use_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Users".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);

    let saved_groups = mock_repo.get_saved_groups().await;
    assert_eq!(saved_groups[0].tags.len(), 0);
}

#[tokio::test]
async fn test_create_group_multiple_tags() {
    let mock_repo = Arc::new(MockGroupRepository::new());
    let mock_uow = Arc::new(MockCreateGroupUnitOfWork::new(mock_repo.clone()));
    let use_case = CreateGroupUseCase::new(mock_uow);

    let cmd = CreateGroupCommand {
        group_name: "Leads".to_string(),
        tags: vec!["lead".to_string(), "senior".to_string(), "manager".to_string()],
    };

    let result = use_case.execute(cmd).await;

    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"lead".to_string()));
    assert!(view.tags.contains(&"senior".to_string()));
    assert!(view.tags.contains(&"manager".to_string()));

    let saved_groups = mock_repo.get_saved_groups().await;
    assert_eq!(saved_groups[0].tags,view.tags);
}

#[cfg(test)]
mod tests {
    use super::super::dto::{CreateGroupCommand, GroupView};
    use super::super::error::CreateGroupError;
    use super::super::ports::{CreateGroupPort, HrnGenerator};
    use super::super::use_case::CreateGroupUseCase;
    use crate::internal::domain::Group;
    use kernel::Hrn;
    use std::sync::Arc;

    // Mock implementation of CreateGroupPort
    struct MockGroupPersister {
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl CreateGroupPort for MockGroupPersister {
async fn save_group(&self, _group: &Group) -> Result<(), CreateGroupError> {
            if self.should_fail {
                Err(CreateGroupError::PersistenceError("Failed to save group".to_string()))
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
        fn new_group_hrn(&self, _name: &str) -> Hrn {
            self.hrn.clone()
        }
    }

    #[tokio::test]
async fn test_create_group_success() {
        // Arrange
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
       );
        
        let persister = Arc::new(MockGroupPersister { should_fail: false });
        let hrn_generator = Arc::new(MockHrnGenerator { hrn: hrn.clone() });
        let use_case = CreateGroupUseCase::new(persister, hrn_generator);
        
        let command= CreateGroupCommand {
            group_name: "Test Group".to_string(),
            tags: vec!["test".to_string()],
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
        let group_view = result.unwrap();
        assert_eq!(group_view.hrn, hrn.to_string());
        assert_eq!(group_view.name, "Test Group");
        assert_eq!(group_view.tags, vec!["test".to_string()]);
    }

    #[tokio::test]
    async fn test_create_group_persistence_error() {
        // Arrange
       let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
        );
        
        let persister = Arc::new(MockGroupPersister{ should_fail: true});
        let hrn_generator = Arc::new(MockHrnGenerator { hrn });
        let use_case = CreateGroupUseCase::new(persister, hrn_generator);
        
        let command = CreateGroupCommand {
            group_name: "Test Group".to_string(),
            tags: vec![],
        };

// Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            CreateGroupError::PersistenceError(_) => (),
            _ => panic!("Expected PersistenceError"),
        }
    }
}
