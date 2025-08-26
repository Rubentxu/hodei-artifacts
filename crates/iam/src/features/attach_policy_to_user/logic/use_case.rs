use crate::application::ports::{UserRepository, PolicyRepository};
use crate::error::IamError;
use crate::features::attach_policy_to_user::command::AttachPolicyToUserCommand;

pub struct AttachPolicyToUserUseCase<'a> {
    user_repository: &'a dyn UserRepository,
    policy_repository: &'a dyn PolicyRepository,
}

impl<'a> AttachPolicyToUserUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository, policy_repository: &'a dyn PolicyRepository) -> Self {
        Self { user_repository, policy_repository }
    }

    pub async fn execute(&self, command: AttachPolicyToUserCommand) -> Result<(), IamError> {
        let mut user = self.user_repository
            .find_by_id(&command.user_id)
            .await?
            .ok_or(IamError::NotFound)?;

        let policy = self.policy_repository
            .find_by_id(&command.policy_id)
            .await?
            .ok_or(IamError::NotFound)?;

        user.policies.push(policy.id);

        self.user_repository.save(&user).await
    }
}
