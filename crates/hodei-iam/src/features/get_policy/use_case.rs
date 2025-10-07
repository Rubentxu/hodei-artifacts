//! Use Case: Get Policy

use std::sync::Arc;
use tracing::{debug, info};

use super::dto::{GetPolicyQuery, PolicyView};
use super::error::GetPolicyError;
use super::ports::PolicyReader;

/// Caso de uso: Obtener una política IAM por su HRN
pub struct GetPolicyUseCase<R>
where
    R: PolicyReader,
{
    reader: Arc<R>,
}

impl<R> GetPolicyUseCase<R>
where
    R: PolicyReader,
{
    /// Crea una nueva instancia del caso de uso
    pub fn new(reader: Arc<R>) -> Self {
        Self { reader }
    }

    /// Ejecuta el caso de uso
    pub async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError> {
        info!("Getting policy: {}", query.policy_hrn);

        // Validar que el HRN sea de tipo Policy
        if query.policy_hrn.resource_type() != "Policy" {
            return Err(GetPolicyError::InvalidHrn(format!(
                "Expected Policy HRN, got: {}",
                query.policy_hrn.resource_type()
            )));
        }

        // Obtener la política usando el reader
        let policy = self.reader.get_by_hrn(&query.policy_hrn).await?;

        debug!("Policy retrieved successfully: {}", policy.hrn);

        Ok(policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    #[tokio::test]
    async fn test_get_policy_success() {
        use crate::features::get_policy::mocks::MockPolicyReader;

        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "test-policy".to_string(),
        );

        let policy = PolicyView {
            hrn: hrn.clone(),
            name: "Test Policy".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("A test policy".to_string()),
        };

        let reader = MockPolicyReader::with_policy(policy.clone());
        let use_case = GetPolicyUseCase::new(Arc::new(reader));

        let query = GetPolicyQuery {
            policy_hrn: hrn.clone(),
        };

        let result = use_case.execute(query).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, hrn);
        assert_eq!(retrieved.name, "Test Policy");
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        use crate::features::get_policy::mocks::MockPolicyReader;

        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "nonexistent".to_string(),
        );

        let reader = MockPolicyReader::empty();
        let use_case = GetPolicyUseCase::new(Arc::new(reader));

        let query = GetPolicyQuery {
            policy_hrn: hrn,
        };

        let result = use_case.execute(query).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetPolicyError::PolicyNotFound(_) => {}
            e => panic!("Expected PolicyNotFound, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_get_policy_invalid_hrn_type() {
        use crate::features::get_policy::mocks::MockPolicyReader;

        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(), // Wrong type
            "test-user".to_string(),
        );

        let reader = MockPolicyReader::empty();
        let use_case = GetPolicyUseCase::new(Arc::new(reader));

        let query = GetPolicyQuery {
            policy_hrn: hrn,
        };

        let result = use_case.execute(query).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetPolicyError::InvalidHrn(_) => {}
            e => panic!("Expected InvalidHrn, got: {:?}", e),
        }
    }
}

