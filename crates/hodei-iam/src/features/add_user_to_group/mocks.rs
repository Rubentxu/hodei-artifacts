use super::ports::{AddUserToGroupRepositories, AddUserToGroupUnitOfWork};
use crate::shared::application::ports::{
    GroupRepository, GroupRepositoryError, UserRepository, UserRepositoryError,
};
use crate::shared::domain::{Group, User};
use kernel::Hrn;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::{Arc, Mutex};

/// Mock implementation of AddUserToGroupUnitOfWork for testing
pub struct MockAddUserToGroupUnitOfWork {
    user_repository: Arc<MockUserRepository>,
    group_repository: Arc<MockGroupRepository>,
    transaction_state: Arc<Mutex<TransactionState>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransactionState {
    NotStarted,
    Active,
    Committed,
    RolledBack,
}

impl MockAddUserToGroupUnitOfWork {
    pub fn new(
        user_repository: Arc<MockUserRepository>,
        group_repository: Arc<MockGroupRepository>,
    ) -> Self {
        Self {
            user_repository,
            group_repository,
            transaction_state: Arc::new(Mutex::new(TransactionState::NotStarted)),
        }
    }

    pub fn transaction_state(&self) -> TransactionState {
        self.transaction_state.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl AddUserToGroupUnitOfWork for MockAddUserToGroupUnitOfWork {
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut state = self.transaction_state.lock().unwrap();
        *state = TransactionState::Active;
        Ok(())
    }

    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut state = self.transaction_state.lock().unwrap();
        if *state != TransactionState::Active {
            return Err("Transaction not active".into());
        }
        *state = TransactionState::Committed;
        Ok(())
    }

    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let mut state = self.transaction_state.lock().unwrap();
        if *state != TransactionState::Active {
            return Err("Transaction not active".into());
        }
        *state = TransactionState::RolledBack;
        self.user_repository.clear();
        self.group_repository.clear();
        Ok(())
    }

    fn repositories(&self) -> AddUserToGroupRepositories {
        AddUserToGroupRepositories::new(
            self.user_repository.clone() as Arc<dyn UserRepository>,
            self.group_repository.clone() as Arc<dyn GroupRepository>,
        )
    }
}

/// Mock UserRepository for testing
pub struct MockUserRepository {
    users: Arc<Mutex<HashMap<String, User>>>,
}

impl MockUserRepository {
    pub fn new() -> Self {
        Self {
            users: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_user(self, user: User) -> Self {
        self.users
            .lock()
            .unwrap()
            .insert(user.hrn.to_string(), user);
        self
    }

    pub fn clear(&self) {
        self.users.lock().unwrap().clear();
    }
}

impl Default for MockUserRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl UserRepository for MockUserRepository {
    async fn save(&self, user: &User) -> Result<(), UserRepositoryError> {
        self.users
            .lock()
            .unwrap()
            .insert(user.hrn.to_string(), user.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
        Ok(self.users.lock().unwrap().get(&hrn.to_string()).cloned())
    }

    async fn find_all(&self) -> Result<Vec<User>, UserRepositoryError> {
        Ok(self.users.lock().unwrap().values().cloned().collect())
    }
}

/// Mock GroupRepository for testing
pub struct MockGroupRepository {
    groups: Arc<Mutex<HashMap<String, Group>>>,
}

impl MockGroupRepository {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_group(self, group: Group) -> Self {
        self.groups
            .lock()
            .unwrap()
            .insert(group.hrn.to_string(), group);
        self
    }

    pub fn clear(&self) {
        self.groups.lock().unwrap().clear();
    }
}

impl Default for MockGroupRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl GroupRepository for MockGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), GroupRepositoryError> {
        self.groups
            .lock()
            .unwrap()
            .insert(group.hrn.to_string(), group.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, GroupRepositoryError> {
        Ok(self.groups.lock().unwrap().get(&hrn.to_string()).cloned())
    }

    async fn find_all(&self) -> Result<Vec<Group>, GroupRepositoryError> {
        Ok(self.groups.lock().unwrap().values().cloned().collect())
    }
}
