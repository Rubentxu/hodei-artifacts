use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::delete_user::command::DeleteUserCommand;
use crate::features::delete_user::logic::use_case::DeleteUserUseCase;

pub async fn handle_delete_user(
    user_repository: &dyn UserRepository,
    command: DeleteUserCommand,
) -> Result<(), IamError> {
    let use_case = DeleteUserUseCase::new(user_repository);
    use_case.execute(command).await
}
