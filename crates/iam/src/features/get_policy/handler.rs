use crate::application::ports::PolicyRepository;
use crate::error::IamError;
use crate::domain::Policy;
use crate::features::get_policy::query::GetPolicyQuery;
use crate::features::get_policy::logic::use_case::GetPolicyUseCase;

pub async fn handle_get_policy(
    policy_repository: &dyn PolicyRepository,
    query: GetPolicyQuery,
) -> Result<Policy, IamError> {
    let use_case = GetPolicyUseCase::new(policy_repository);
    use_case.execute(query).await
}
