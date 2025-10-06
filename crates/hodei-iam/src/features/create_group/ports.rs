use crate::internal::application::ports::GroupRepository;
use std::error::Error as StdError;

/// Unit of Work for the create_group feature
///
/// Ensures transactional integrity when creating a new group.
#[async_trait::async_trait]
pub trait CreateGroupUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Commit the transaction
    async fn commit(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Rollback the transaction
    async fn rollback(&self) -> Result<(), Box<dyn StdError + Send + Sync>>;

    /// Access repositories within the transaction context
    fn repositories(&self) -> CreateGroupRepositories;
}

/// Repository bundle for create_group feature
#[derive(Clone)]
pub struct CreateGroupRepositories {
    pub group_repository: std::sync::Arc<dyn GroupRepository>,
}

impl CreateGroupRepositories {
    pub fn new(group_repository: std::sync::Arc<dyn GroupRepository>) -> Self {
        Self { group_repository }
    }
}
