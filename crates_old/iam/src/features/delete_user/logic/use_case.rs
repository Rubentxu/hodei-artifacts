use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::delete_user::command::DeleteUserCommand;

pub struct DeleteUserUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> DeleteUserUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, command: DeleteUserCommand) -> Result<(), IamError> {
        self.user_repository.delete(&command.id).await
    }
}
