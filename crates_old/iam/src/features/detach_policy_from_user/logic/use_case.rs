use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::detach_policy_from_user::command::DetachPolicyFromUserCommand;

pub struct DetachPolicyFromUserUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> DetachPolicyFromUserUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, command: DetachPolicyFromUserCommand) -> Result<(), IamError> {
        let mut user = self.user_repository
            .find_by_id(&command.user_id)
            .await?
            .ok_or(IamError::NotFound)?;

        user.policies.retain(|p| p != &command.policy_id);

        self.user_repository.save(&user).await
    }
}
