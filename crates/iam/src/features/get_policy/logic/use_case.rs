use crate::application::ports::PolicyRepository;
use crate::domain::Policy;
use crate::error::IamError;
use crate::features::get_policy::query::GetPolicyQuery;

pub struct GetPolicyUseCase<'a> {
    policy_repository: &'a dyn PolicyRepository,
}

impl<'a> GetPolicyUseCase<'a> {
    pub fn new(policy_repository: &'a dyn PolicyRepository) -> Self {
        Self { policy_repository }
    }

    pub async fn execute(&self, query: GetPolicyQuery) -> Result<Policy, IamError> {
        self.policy_repository
            .find_by_id(&query.id)
            .await?
            .ok_or(IamError::NotFound)
    }
}
