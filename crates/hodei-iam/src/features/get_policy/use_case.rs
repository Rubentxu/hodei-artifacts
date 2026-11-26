//! Use Case: Get Policy

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

use super::dto::{GetPolicyQuery, PolicyView};
use super::error::GetPolicyError;
use super::ports::{GetPolicyUseCasePort, PolicyReader};
use kernel::Hrn;

/// Use case: Get an IAM policy by its HRN
pub struct GetPolicyUseCase {
    reader: Arc<dyn PolicyReader>,
}

impl GetPolicyUseCase {
    /// Creates a new instance of the use case
    pub fn new(reader: Arc<dyn PolicyReader>) -> Self {
        Self { reader }
    }

    /// Executes the use case
    pub async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError> {
        info!("Getting policy: {}", query.policy_hrn);

        // Validate that the HRN is of type Policy
        if query.policy_hrn.resource_type() != "Policy" {
            return Err(GetPolicyError::InvalidHrn(format!(
                "Expected Policy HRN, got: {}",
                query.policy_hrn.resource_type()
            )));
        }

        // Get the policy using the reader
        let policy = self.reader.get_by_hrn(&query.policy_hrn).await?;

        debug!("Policy retrieved successfully: {}", policy.hrn);

        Ok(policy)
    }
}

// Implement PolicyReader trait for the use case to enable trait object usage
#[async_trait]
impl PolicyReader for GetPolicyUseCase {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        let query = GetPolicyQuery {
            policy_hrn: hrn.clone(),
        };
        self.execute(query).await
    }
}

#[async_trait]
impl GetPolicyUseCasePort for GetPolicyUseCase {
    async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError> {
        self.execute(query).await
    }
}
