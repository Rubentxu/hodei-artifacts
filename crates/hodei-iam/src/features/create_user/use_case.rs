use super::dto::{CreateUserCommand, UserView};
use super::error::CreateUserError;
use super::ports::{CreateUserPort, HrnGenerator};
use crate::internal::domain::User;
use std::sync::Arc;

/// Use case for creating a new user
///
/// This use case orchestrates the user creation process:
/// 1. Generates a new HRN for the user
/// 2. Creates a User entity
/// 3. Persists the user through the port
/// 4. Returns a UserView DTO
pub struct CreateUserUseCase<P: CreateUserPort, G: HrnGenerator> {
    persister: Arc<P>,
    hrn_generator: Arc<G>,
}

impl<P: CreateUserPort, G: HrnGenerator> CreateUserUseCase<P, G> {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    /// * `persister` - Implementation of CreateUserPort for persistence
    /// * `hrn_generator` - Implementation of HrnGenerator for HRN generation
    pub fn new(persister: Arc<P>, hrn_generator: Arc<G>) -> Self {
        Self {
            persister,
            hrn_generator,
        }
    }

    /// Execute the create user use case
    ///
    /// # Arguments
    /// * `cmd` - CreateUserCommand containing user details
    ///
    /// # Returns
    /// * Ok(UserView) if the user was created successfully
    /// * Err(CreateUserError) if there was an error
    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, CreateUserError> {
        // Generate a unique HRN using the HRN generator
        let hrn = self.hrn_generator.new_user_hrn(&cmd.name);
        
        // Create the user domain entity
        let user = User::new(hrn.clone(), cmd.name, cmd.email);
        
        // Persist the user
        self.persister.save_user(&user).await?;
        
        // Return the view
        Ok(UserView {
            hrn: hrn.to_string(),
            name: user.name,
            email: user.email,
            groups: Vec::new(), // New user has no groups
            tags: user.tags,
        })
    }
}