// crates/iam/src/features/get_policy/dto.rs

use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use cedar_policy::PolicyId;
use serde::{Deserialize, Serialize};

/// Query to get a policy by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    /// ID of the policy to retrieve
    pub id: PolicyId,
}

/// Response containing the requested policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyResponse {
    /// The requested policy
    pub policy: Policy,
}

impl GetPolicyQuery {
    /// Create a new get policy query
    pub fn new(id: PolicyId) -> Self {
        Self { id }
    }

    /// Validate the query
    pub fn validate(&self) -> Result<(), IamError> {
        // Basic validation - PolicyId is already validated during construction
        // Additional business rules could be added here
        Ok(())
    }
}

impl GetPolicyResponse {
    /// Create a new get policy response
    pub fn new(policy: Policy) -> Self {
        Self { policy }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::{Policy, PolicyMetadata, PolicyStatus};
    use shared::hrn::Hrn;
    use time::OffsetDateTime;

    #[test]
    fn test_get_policy_query_new() {
        let policy_id = PolicyId(Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"));
        let query = GetPolicyQuery::new(policy_id.clone());

        assert_eq!(query.id, policy_id);
    }

    #[test]
    fn test_get_policy_query_validate() {
        let policy_id = PolicyId(Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"));
        let query = GetPolicyQuery::new(policy_id);

        assert!(query.validate().is_ok());
    }

    #[test]
    fn test_get_policy_response_new() {
        let policy_id = PolicyId(Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"));
        let policy = Policy {
            id: policy_id,
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
            metadata: PolicyMetadata {
                created_at: OffsetDateTime::now_utc(),
                created_by: "user_123".to_string(),
                updated_at: OffsetDateTime::now_utc(),
                updated_by: "user_123".to_string(),
                version: 1,
                tags: vec!["test".to_string()],
            },
        };

        let response = GetPolicyResponse::new(policy.clone());
        assert_eq!(response.policy.id, policy.id);
        assert_eq!(response.policy.name, policy.name);
    }
}
