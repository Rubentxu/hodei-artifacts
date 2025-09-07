use crate::application::ports::{PolicyRepository, PolicyValidator};
use crate::domain::{Policy, PolicyStatus};
use crate::error::IamError;
use crate::features::create_policy::command::CreatePolicyCommand;
use crate::features::create_policy::logic::validate::validate_command;
use cedar_policy::PolicyId;
use uuid::Uuid;

pub struct CreatePolicyUseCase<'a> {
    policy_repository: &'a dyn PolicyRepository,
    policy_validator: &'a dyn PolicyValidator,
}

impl<'a> CreatePolicyUseCase<'a> {
    pub fn new(policy_repository: &'a dyn PolicyRepository, policy_validator: &'a dyn PolicyValidator) -> Self {
        Self { policy_repository, policy_validator }
    }

    pub async fn execute(&self, cmd: CreatePolicyCommand) -> Result<PolicyId, IamError> {
        validate_command(&cmd, self.policy_validator)?;

        let new_policy = Policy {
            id: PolicyId::new(Uuid::new_v4().to_string()),
            name: cmd.name,
            description: cmd.description,
            version: 1, // Initial version
            content: cmd.content,
            status: PolicyStatus::Active, // Default to active
        };

        self.policy_repository.save(&new_policy).await?;

        Ok(new_policy.id)
    }
}
