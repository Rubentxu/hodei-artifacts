use crate::application::ports::PolicyRepository;
use crate::error::IamError;
use crate::domain::Policy;
use crate::features::list_policies::query::ListPoliciesQuery;
use crate::features::list_policies::logic::use_case::ListPoliciesUseCase;

pub async fn handle_list_policies(
    policy_repository: &dyn PolicyRepository,
    _query: ListPoliciesQuery,
) -> Result<Vec<Policy>, IamError> {
    let use_case = ListPoliciesUseCase::new(policy_repository);
    use_case.execute().await
}
