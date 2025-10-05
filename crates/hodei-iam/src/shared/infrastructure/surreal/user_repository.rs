use crate::shared::application::ports::{UserRepository, UserRepositoryError};
use crate::shared::domain::User;
use async_trait::async_trait;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use tracing::error;

/// SurrealDB implementation of UserRepository
pub struct SurrealUserRepository {
    db: Surreal<Any>,
}

impl SurrealUserRepository {
    /// Create a new SurrealUserRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserRepository for SurrealUserRepository {
    async fn save(&self, user: &User) -> Result<(), UserRepositoryError> {
        let thing: RecordId = ("users", user.hrn.to_string()).try_into().map_err(|e| {
            error!("Failed to create RecordId for user {}: {}", user.hrn, e);
            UserRepositoryError::InvalidHrn(user.hrn.to_string())
        })?;

        let _: surrealdb::opt::IntoRecordId =
            self.db.create(thing).content(user).await.map_err(|e| {
                error!("Database error while saving user {}: {}", user.hrn, e);
                UserRepositoryError::DatabaseError(e.to_string())
            })?;

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
        let thing: RecordId = ("users", hrn.to_string()).try_into().map_err(|e| {
            error!("Failed to create RecordId for hrn {}: {}", hrn, e);
            UserRepositoryError::InvalidHrn(hrn.to_string())
        })?;

        let user: Option<User> = self.db.select(thing).await.map_err(|e| {
            error!("Database error while finding user {}: {}", hrn, e);
            UserRepositoryError::DatabaseError(e.to_string())
        })?;

        Ok(user)
    }

    async fn find_all(&self) -> Result<Vec<User>, UserRepositoryError> {
        let users: Vec<User> = self.db.select("users").await.map_err(|e| {
            error!("Database error while finding all users: {}", e);
            UserRepositoryError::DatabaseError(e.to_string())
        })?;

        Ok(users)
    }
}
