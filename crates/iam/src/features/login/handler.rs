use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::login::command::{LoginCommand, LoginResponse};
use crate::features::login::logic::use_case::LoginUseCase;

pub async fn handle_login(
    user_repository: &dyn UserRepository,
    command: LoginCommand,
) -> Result<LoginResponse, IamError> {
    let use_case = LoginUseCase::new(user_repository);
    use_case.execute(command).await
}
