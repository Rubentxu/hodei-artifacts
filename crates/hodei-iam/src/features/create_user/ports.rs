use crate::internal::domain::User;
use async_trait::async_trait;
use kernel::Hrn;
use super::error::CreateUserError;

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
    /// * `user` - The user entity to save
    ///
    /// # Returns
    /// * `Ok(())` if the user was saved successfully
    /// * `Err(CreateUserError)` if there was an error saving the user
    async fn save_user(&self, user: &User) -> Result<(), CreateUserError>;
}

/// Port for generating HRNs
///
/// This port abstracts HRN generation, allowing different implementations
/// (e.g., UUID-based, sequential, etc.)
pub trait HrnGenerator: Send + Sync {
    /// Generate a new HRN for a user
    ///
    /// # Arguments
    /// * `name` - The name of the user (used for HRN generation)
    ///
    /// # Returns
    /// * A new HRN for the user
    fn new_user_hrn(&self, name: &str) -> Hrn;
}