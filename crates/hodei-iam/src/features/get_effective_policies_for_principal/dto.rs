//! DTOs for the get_effective_policies_for_principal feature
//!
//! This module defines the data transfer objects for retrieving effective
//! IAM policies for a principal, maintaining decoupling from Cedar types.

use serde::{Deserialize, Serialize};

/// Query to get effective IAM policies for a principal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectivePoliciesQuery {
    /// HRN of the principal (user, service account, etc.)
    pub principal_hrn: String,
}

/// Response containing effective IAM policies as Cedar policy strings
///
/// This is the PUBLIC interface - does not expose internal entities or Cedar types.
/// Policies are returned as strings to maintain architectural boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePoliciesResponse {
    /// List of Cedar policy document strings
    /// This includes:
    /// - Direct policies attached to the user
    /// - Policies from all groups the user belongs to
    /// - Policies from roles assigned to the user
    pub policies: Vec<String>,

    /// HRN of the principal (for logging/debugging)
    pub principal_hrn: String,

    /// Number of policies included (for observability)
    pub policy_count: usize,
}

impl EffectivePoliciesResponse {
    /// Create a new response with the given policies and principal HRN
    pub fn new(policies: Vec<String>, principal_hrn: String) -> Self {
        let policy_count = policies.len();
        Self {
            policies,
            principal_hrn,
            policy_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_creation() {
        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:iam:user:alice".to_string(),
        };

        assert_eq!(query.principal_hrn, "hrn:iam:user:alice");
    }

    #[test]
    fn test_response_creation() {
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "forbid(principal, action, resource) when { false };".to_string(),
        ];

        let response =
            EffectivePoliciesResponse::new(policies.clone(), "hrn:iam:user:alice".to_string());

        assert_eq!(response.policies, policies);
        assert_eq!(response.principal_hrn, "hrn:iam:user:alice");
        assert_eq!(response.policy_count, 2);
    }

    #[test]
    fn test_response_with_empty_policies() {
        let response = EffectivePoliciesResponse::new(vec![], "hrn:iam:user:bob".to_string());

        assert_eq!(response.policies.len(), 0);
        assert_eq!(response.policy_count, 0);
    }

    #[test]
    fn test_query_serialization() {
        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:iam:user:charlie".to_string(),
        };

        let json = serde_json::to_string(&query).expect("serialize");
        assert!(json.contains("charlie"));
    }

    #[test]
    fn test_response_serialization() {
        let response = EffectivePoliciesResponse::new(
            vec!["permit(principal, action, resource);".to_string()],
            "hrn:iam:user:dave".to_string(),
        );

        let json = serde_json::to_string(&response).expect("serialize");
        assert!(json.contains("dave"));
        assert!(json.contains("permit"));
    }
}
