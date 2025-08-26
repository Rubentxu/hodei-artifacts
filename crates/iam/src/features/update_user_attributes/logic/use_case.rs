use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::update_user_attributes::command::UpdateUserAttributesCommand;
use crate::features::update_user_attributes::logic::validate::validate_command;

pub struct UpdateUserAttributesUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> UpdateUserAttributesUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, cmd: UpdateUserAttributesCommand) -> Result<(), IamError> {
        validate_command(&cmd)?;

        let mut user = self.user_repository.find_by_id(&cmd.user_id).await?
            .ok_or(IamError::NotFound)?;

        user.attributes = cmd.attributes;

        self.user_repository.save(&user).await?;

        Ok(())
    }
}
