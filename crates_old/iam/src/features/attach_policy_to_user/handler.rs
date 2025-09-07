use crate::application::ports::{UserRepository, PolicyRepository};
use crate::error::IamError;
use crate::features::attach_policy_to_user::command::AttachPolicyToUserCommand;
use crate::features::attach_policy_to_user::logic::use_case::AttachPolicyToUserUseCase;

pub async fn handle_attach_policy_to_user(
    user_repository: &dyn UserRepository,
    policy_repository: &dyn PolicyRepository,
    command: AttachPolicyToUserCommand,
) -> Result<(), IamError> {
    let use_case = AttachPolicyToUserUseCase::new(user_repository, policy_repository);
    use_case.execute(command).await
}
