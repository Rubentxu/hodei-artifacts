use crate::application::ports::PolicyRepository;
use crate::error::IamError;
use crate::features::delete_policy::command::DeletePolicyCommand;

pub struct DeletePolicyUseCase<'a> {
    policy_repository: &'a dyn PolicyRepository,
}

impl<'a> DeletePolicyUseCase<'a> {
    pub fn new(policy_repository: &'a dyn PolicyRepository) -> Self {
        Self { policy_repository }
    }

    pub async fn execute(&self, command: DeletePolicyCommand) -> Result<(), IamError> {
        self.policy_repository.delete(&command.id).await
    }
}
