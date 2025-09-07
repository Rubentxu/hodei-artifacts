use crate::application::ports::PolicyRepository;
use crate::domain::Policy;
use crate::error::IamError;

pub struct ListPoliciesUseCase<'a> {
    policy_repository: &'a dyn PolicyRepository,
}

impl<'a> ListPoliciesUseCase<'a> {
    pub fn new(policy_repository: &'a dyn PolicyRepository) -> Self {
        Self { policy_repository }
    }

    pub async fn execute(&self) -> Result<Vec<Policy>, IamError> {
        self.policy_repository.find_all().await
    }
}
