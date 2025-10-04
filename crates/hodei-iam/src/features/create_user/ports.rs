use crate::shared::application::ports::UserRepository;
use std::error::Error as StdError;

/// Unit of Work for the create_user feature
///
/// Ensures transactional integrity when creating a new user.
#[async_trait::async_trait]
pub trait CreateUserUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Commit the transaction
    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Rollback the transaction
    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Access repositories within the transaction context
    fn repositories(&self) -> CreateUserRepositories;
}

/// Repository bundle for create_user feature
#[derive(Clone)]
pub struct CreateUserRepositories {
    pub user_repository: std::sync::Arc<dyn UserRepository>,
}

impl CreateUserRepositories {
    pub fn new(user_repository: std::sync::Arc<dyn UserRepository>) -> Self {
        Self { user_repository }
    }
}
