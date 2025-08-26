use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::detach_policy_from_user::command::DetachPolicyFromUserCommand;
use crate::features::detach_policy_from_user::logic::use_case::DetachPolicyFromUserUseCase;

pub async fn handle_detach_policy_from_user(
    user_repository: &dyn UserRepository,
    command: DetachPolicyFromUserCommand,
) -> Result<(), IamError> {
    let use_case = DetachPolicyFromUserUseCase::new(user_repository);
    use_case.execute(command).await
}
