use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use shared::UserId;
use crate::application::ports::UserRepository;
use crate::domain::User;
use crate::error::IamError;



pub struct MockUserRepository {
    users: Arc<Mutex<HashMap<UserId, User>>>,
}

impl MockUserRepository {
    pub fn new() -> Self {
        Self { users: Arc::new(Mutex::new(HashMap::new())) }
    }
}

#[async_trait]
impl UserRepository for MockUserRepository {
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, IamError> {
        let users = self.users.lock().unwrap();
        Ok(users.get(id).cloned())
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, IamError> {
        let users = self.users.lock().unwrap();
        Ok(users.values().find(|u| u.username == username).cloned())
    }

    async fn save(&self, user: &User) -> Result<(), IamError> {
        let mut users = self.users.lock().unwrap();
        users.insert(user.id.clone(), user.clone());
        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<User>, IamError> {
        let users = self.users.lock().unwrap();
        Ok(users.values().cloned().collect())
    }

    async fn delete(&self, id: &UserId) -> Result<(), IamError> {
        let mut users = self.users.lock().unwrap();
        users.remove(id);
        Ok(())
    }
}
