use super::dto::{CreateUserCommand, UserView};
use crate::shared::{
    application::ports::UserRepository,
    domain::User,
};
use policies::shared::domain::hrn::Hrn;
/// Use case for creating a new user

use std::sync::Arc;

pub struct CreateUserUseCase {
    repo: Arc<dyn UserRepository>,
}

impl CreateUserUseCase {
    pub fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let user_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            user_id,
        );

        // Create the user domain entity
        let mut user = User::new(hrn, cmd.name.clone(), cmd.email.clone());
        user.tags = cmd.tags.clone();

        // Persist the user
        self.repo.save(&user).await?;

        // Return the view
        Ok(UserView {
            hrn: user.hrn.to_string(),
            name: user.name,
            email: user.email,
            groups: Vec::new(),
            tags: user.tags,
        })
    }
}
