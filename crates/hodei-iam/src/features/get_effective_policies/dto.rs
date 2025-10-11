//! DTOs for the get_effective_policies_for_principal feature
//!
//! This module defines the data transfer objects for retrieving effective
//! IAM policies for a principal, using kernel types for strong typing.

use kernel::domain::policy::HodeiPolicySet;
use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Data Transfer Object for user lookup operations
///
/// This DTO is used to transfer user data from the persistence layer
/// without exposing the internal User domain entity.
#[derive(Debug, Clone)]
pub struct UserLookupDto {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub group_hrns: Vec<String>,
    pub tags: Vec<String>,
}

impl UserLookupDto {
    /// Create a new UserLookupDto
    pub fn new(hrn: impl Into<String>, name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            hrn: hrn.into(),
            name: name.into(),
            email: email.into(),
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// Data Transfer Object for group lookup operations
///
/// This DTO is used to transfer group data from the persistence layer
/// without exposing the internal Group domain entity.
#[derive(Debug, Clone)]
pub struct GroupLookupDto {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

impl GroupLookupDto {
    /// Create a new GroupLookupDto
    pub fn new(hrn: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            hrn: hrn.into(),
            name: name.into(),
            tags: Vec::new(),
        }
    }
}

/// Query to get effective IAM policies for a principal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectivePoliciesQuery {
    /// HRN of the principal (user, serviceaccount, etc.)
    pub principal_hrn: String,
}

impl ActionTrait for GetEffectivePoliciesQuery {
    fn name() -> &'static str {
        "GetEffectivePolicies"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Policy".to_string()
    }
}

/// Response containing effective IAM policies as a HodeiPolicySet
///
/// This is the PUBLIC interface - returns kernel types for strong typing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePoliciesResponse {
    /// Set of effective policies
    /// This includes:
    /// - Direct policies attached to the user
    /// - Policies from all groups the user belongs to
    pub policies: HodeiPolicySet,

    /// HRN of the principal (for logging/debugging)
    pub principal_hrn: String,
}

impl EffectivePoliciesResponse {
    /// Create a new response with the given policies and principal HRN
    pub fn new(policies: HodeiPolicySet, principal_hrn: impl Into<String>) -> Self {
        Self {
            policies,
            principal_hrn: principal_hrn.into(),
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
        let policies = HodeiPolicySet::new(vec![]);
        let response = EffectivePoliciesResponse::new(policies, "hrn:iam:user:alice".to_string());

        assert_eq!(response.policies.len(), 0);
        assert_eq!(response.principal_hrn, "hrn:iam:user:alice");
    }

    #[test]
    fn test_response_with_empty_policies() {
        let response = EffectivePoliciesResponse::new(
            HodeiPolicySet::new(vec![]),
            "hrn:iam:user:bob".to_string(),
        );

        assert_eq!(response.policies.len(), 0);
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
            HodeiPolicySet::new(vec![]),
            "hrn:iam:user:dave".to_string(),
        );

        let json = serde_json::to_string(&response).expect("serialize");
        assert!(json.contains("dave"));
    }
}
