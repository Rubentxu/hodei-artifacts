use super::dto::{CreateUserCommand, UserPersistenceDto, UserView};
use super::error::CreateUserError;

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

/// Port for the CreateUser use case
///
/// This port defines the contract for executing the create user use case.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the execute method needed by external callers.
#[async_trait]
pub trait CreateUserUseCasePort: Send + Sync {
    /// Execute the create user use case
    ///
    /// # Arguments
    /// * `command` - The create user command containing user details
    ///
    /// # Returns
    /// * `Ok(UserView)` if the user was created successfully
    /// * `Err(CreateUserError)` if there was an error creating the user
    async fn execute(&self, command: CreateUserCommand) -> Result<UserView, CreateUserError>;
}
