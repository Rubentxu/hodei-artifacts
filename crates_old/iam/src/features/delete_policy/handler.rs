use crate::application::ports::PolicyRepository;
use crate::error::IamError;
use crate::features::delete_policy::command::DeletePolicyCommand;
use crate::features::delete_policy::logic::use_case::DeletePolicyUseCase;

pub async fn handle_delete_policy(
    policy_repository: &dyn PolicyRepository,
    command: DeletePolicyCommand,
) -> Result<(), IamError> {
    let use_case = DeletePolicyUseCase::new(policy_repository);
    use_case.execute(command).await
}
