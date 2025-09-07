use crate::application::ports::{PolicyRepository, PolicyValidator};
use crate::error::IamError;
use crate::features::create_policy::command::CreatePolicyCommand;
use crate::features::create_policy::logic::use_case::CreatePolicyUseCase;
use cedar_policy::PolicyId;

// This is the new handle function that will be used by the IamApi
pub async fn handle_create_policy(
    policy_repository: &dyn PolicyRepository,
    policy_validator: &dyn PolicyValidator,
    command: CreatePolicyCommand,
) -> Result<PolicyId, IamError> {
    let use_case = CreatePolicyUseCase::new(policy_repository, policy_validator);
    use_case.execute(command).await
}
