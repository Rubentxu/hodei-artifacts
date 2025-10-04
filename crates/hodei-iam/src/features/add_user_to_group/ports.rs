use crate::shared::application::ports::{GroupRepository, UserRepository};
use std::error::Error as StdError;

/// Unit of Work for the add_user_to_group feature
///
/// Ensures transactional integrity when adding a user to a group.
/// Both the user lookup/update and group validation must succeed or fail atomically.
#[async_trait::async_trait]
pub trait AddUserToGroupUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Commit the transaction
    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Rollback the transaction
    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Access repositories within the transaction context
    fn repositories(&self) -> AddUserToGroupRepositories;
}

/// Repository bundle for add_user_to_group feature
#[derive(Clone)]
pub struct AddUserToGroupRepositories {
    pub user_repository: std::sync::Arc<dyn UserRepository>,
    pub group_repository: std::sync::Arc<dyn GroupRepository>,
}

impl AddUserToGroupRepositories {
    pub fn new(
        user_repository: std::sync::Arc<dyn UserRepository>,
        group_repository: std::sync::Arc<dyn GroupRepository>,
    ) -> Self {
        Self {
            user_repository,
            group_repository,
        }
    }
}
