//! Test Fixtures
//!
//! This module provides reusable test data and fixtures for integration
//! and E2E tests.

use kernel::{HodeiPolicy, Hrn};
use serde_json::json;

/// Sample valid Cedar policy content
pub fn valid_policy_content() -> String {
    r#"permit(
    principal,
    action == Action::"ReadDocument",
    resource
);"#
    .to_string()
}

/// Sample valid Cedar policy with conditions
pub fn valid_policy_with_conditions() -> String {
    r#"permit(
    principal == User::"alice",
    action == Action::"ReadDocument",
    resource
) when {
    resource.owner == principal
};"#
    .to_string()
}

/// Sample valid Cedar policy with multiple actions
pub fn valid_policy_multiple_actions() -> String {
    r#"permit(
    principal,
    action in [Action::"Read", Action::"Write", Action::"Delete"],
    resource
);"#
    .to_string()
}

/// Invalid Cedar policy (syntax error)
pub fn invalid_policy_syntax() -> String {
    "permit(principal action resource".to_string()
}

/// Invalid Cedar policy (semantic error)
pub fn invalid_policy_semantic() -> String {
    r#"permit(
    principal == NonExistentEntityType::"test",
    action,
    resource
);"#
    .to_string()
}

/// Empty policy content
pub fn empty_policy_content() -> String {
    "".to_string()
}

/// Create a test HRN for a policy
pub fn test_policy_hrn(id: &str) -> Hrn {
    Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "test-account".to_string(),
        "Policy".to_string(),
        id.to_string(),
    )
}

/// Create a test HRN for a user
pub fn test_user_hrn(id: &str) -> Hrn {
    Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "test-account".to_string(),
        "User".to_string(),
        id.to_string(),
    )
}

/// Create a test HRN for a group
pub fn test_group_hrn(id: &str) -> Hrn {
    Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "test-account".to_string(),
        "Group".to_string(),
        id.to_string(),
    )
}

/// Create a test HodeiPolicy
pub fn test_policy(id: &str) -> HodeiPolicy {
    HodeiPolicy::new(id.to_string(), valid_policy_content())
}

/// Create a test HodeiPolicy with custom content
pub fn test_policy_with_content(id: &str, content: String) -> HodeiPolicy {
    HodeiPolicy::new(id.to_string(), content)
}

/// Sample policy IDs for testing
pub fn sample_policy_ids() -> Vec<String> {
    vec![
        "allow-read-docs".to_string(),
        "allow-write-docs".to_string(),
        "allow-delete-docs".to_string(),
        "deny-public-access".to_string(),
        "admin-full-access".to_string(),
    ]
}

/// Generate test policies in bulk
pub fn generate_test_policies(count: usize) -> Vec<HodeiPolicy> {
    (0..count)
        .map(|i| test_policy(&format!("test-policy-{}", i)))
        .collect()
}

/// Sample CreatePolicyRequest JSON
pub fn create_policy_request_json(id: &str) -> serde_json::Value {
    json!({
        "policy_id": id,
        "policy_content": valid_policy_content(),
        "description": format!("Test policy {}", id)
    })
}

/// Sample CreatePolicyRequest JSON with invalid content
pub fn create_policy_request_invalid_json(id: &str) -> serde_json::Value {
    json!({
        "policy_id": id,
        "policy_content": invalid_policy_syntax(),
        "description": "Invalid test policy"
    })
}

/// Sample UpdatePolicyRequest JSON
pub fn update_policy_request_json(hrn: &Hrn) -> serde_json::Value {
    json!({
        "policy_hrn": hrn,
        "policy_content": valid_policy_with_conditions(),
        "description": "Updated test policy"
    })
}

/// Sample GetPolicyRequest JSON
pub fn get_policy_request_json(hrn: &Hrn) -> serde_json::Value {
    json!({
        "policy_hrn": hrn
    })
}

/// Sample DeletePolicyRequest JSON
pub fn delete_policy_request_json(hrn: &Hrn) -> serde_json::Value {
    json!({
        "policy_hrn": hrn
    })
}

/// Test pagination scenarios
pub struct PaginationScenario {
    pub total_items: usize,
    pub limit: usize,
    pub offset: usize,
    pub expected_returned: usize,
    pub expected_has_next: bool,
    pub expected_has_previous: bool,
}

/// Common pagination test scenarios
pub fn pagination_scenarios() -> Vec<PaginationScenario> {
    vec![
        // First page
        PaginationScenario {
            total_items: 100,
            limit: 10,
            offset: 0,
            expected_returned: 10,
            expected_has_next: true,
            expected_has_previous: false,
        },
        // Middle page
        PaginationScenario {
            total_items: 100,
            limit: 10,
            offset: 50,
            expected_returned: 10,
            expected_has_next: true,
            expected_has_previous: true,
        },
        // Last page (partial)
        PaginationScenario {
            total_items: 95,
            limit: 10,
            offset: 90,
            expected_returned: 5,
            expected_has_next: false,
            expected_has_previous: true,
        },
        // Single page (all items)
        PaginationScenario {
            total_items: 5,
            limit: 10,
            offset: 0,
            expected_returned: 5,
            expected_has_next: false,
            expected_has_previous: false,
        },
        // Empty result
        PaginationScenario {
            total_items: 0,
            limit: 10,
            offset: 0,
            expected_returned: 0,
            expected_has_next: false,
            expected_has_previous: false,
        },
        // Large limit
        PaginationScenario {
            total_items: 100,
            limit: 100,
            offset: 0,
            expected_returned: 100,
            expected_has_next: false,
            expected_has_previous: false,
        },
    ]
}

/// Error test scenarios
#[derive(Debug, Clone)]
pub struct ErrorScenario {
    pub name: &'static str,
    pub policy_id: String,
    pub policy_content: String,
    pub expected_status: u16,
    pub expected_error_contains: &'static str,
}

/// Common error scenarios for policy creation
pub fn create_policy_error_scenarios() -> Vec<ErrorScenario> {
    vec![
        ErrorScenario {
            name: "empty_policy_id",
            policy_id: "".to_string(),
            policy_content: valid_policy_content(),
            expected_status: 400,
            expected_error_contains: "Policy ID cannot be empty",
        },
        ErrorScenario {
            name: "empty_policy_content",
            policy_id: "test-policy".to_string(),
            policy_content: "".to_string(),
            expected_status: 400,
            expected_error_contains: "Policy content cannot be empty",
        },
        ErrorScenario {
            name: "invalid_syntax",
            policy_id: "test-policy".to_string(),
            policy_content: invalid_policy_syntax(),
            expected_status: 400,
            expected_error_contains: "Invalid policy content",
        },
    ]
}

/// Common error scenarios for policy updates
pub fn update_policy_error_scenarios() -> Vec<ErrorScenario> {
    vec![
        ErrorScenario {
            name: "empty_policy_content",
            policy_id: "existing-policy".to_string(),
            policy_content: "".to_string(),
            expected_status: 400,
            expected_error_contains: "Policy content cannot be empty",
        },
        ErrorScenario {
            name: "invalid_syntax",
            policy_id: "existing-policy".to_string(),
            policy_content: invalid_policy_syntax(),
            expected_status: 400,
            expected_error_contains: "Invalid policy content",
        },
    ]
}

/// Test user credentials
#[derive(Debug, Clone)]
pub struct TestUser {
    pub id: String,
    pub name: String,
    pub email: String,
    pub is_admin: bool,
}

/// Sample test users
pub fn test_users() -> Vec<TestUser> {
    vec![
        TestUser {
            id: "alice".to_string(),
            name: "Alice Admin".to_string(),
            email: "alice@test.com".to_string(),
            is_admin: true,
        },
        TestUser {
            id: "bob".to_string(),
            name: "Bob User".to_string(),
            email: "bob@test.com".to_string(),
            is_admin: false,
        },
        TestUser {
            id: "charlie".to_string(),
            name: "Charlie Reader".to_string(),
            email: "charlie@test.com".to_string(),
            is_admin: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_policy_content_not_empty() {
        assert!(!valid_policy_content().is_empty());
    }

    #[test]
    fn test_invalid_policy_syntax_not_empty() {
        assert!(!invalid_policy_syntax().is_empty());
    }

    #[test]
    fn test_generate_test_policies() {
        let policies = generate_test_policies(5);
        assert_eq!(policies.len(), 5);
    }

    #[test]
    fn test_sample_policy_ids_not_empty() {
        let ids = sample_policy_ids();
        assert!(!ids.is_empty());
        assert!(ids.len() >= 3);
    }

    #[test]
    fn test_pagination_scenarios() {
        let scenarios = pagination_scenarios();
        assert!(!scenarios.is_empty());
        // Verify first scenario
        assert_eq!(scenarios[0].offset, 0);
        assert!(!scenarios[0].expected_has_previous);
    }

    #[test]
    fn test_error_scenarios() {
        let scenarios = create_policy_error_scenarios();
        assert!(!scenarios.is_empty());
        for scenario in scenarios {
            assert!(scenario.expected_status >= 400);
        }
    }

    #[test]
    fn test_test_users() {
        let users = test_users();
        assert!(!users.is_empty());
        assert!(users.iter().any(|u| u.is_admin));
    }
}
