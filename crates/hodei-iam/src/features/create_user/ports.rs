use super::dto::UserPersistenceDto;
use super::error::CreateUserError;
use crate::infrastructure::hrn_generator::HrnGenerator;
use async_trait::async_trait;

/// Port for persisting users
///
/// This port abstracts user persistence operations.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the operations needed by the create_user feature.
#[async_trait]
pub trait CreateUserPort: Send + Sync {
    /// Save a user to the persistence layer
    ///
    /// # Arguments
    /// * `user_dto` - The user data transfer object to save
    ///
    /// # Returns
    /// * `Ok(())` if the user was saved successfully
    /// * `Err(CreateUserError)` if there was an error saving the user
    async fn save_user(&self, user_dto: &UserPersistenceDto) -> Result<(), CreateUserError>;
}
