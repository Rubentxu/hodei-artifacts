use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::create_user::command::CreateUserCommand;
use crate::features::create_user::logic::use_case::CreateUserUseCase;
use shared::UserId;

// This is the new handle function that will be used by the IamApi
pub async fn handle_create_user(
    user_repository: &dyn UserRepository,
    command: CreateUserCommand,
) -> Result<UserId, IamError> {
    let use_case = CreateUserUseCase::new(user_repository);
    use_case.execute(command).await
}
