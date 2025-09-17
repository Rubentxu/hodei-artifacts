//! Unit tests for create_policy event handler
//!
//! This module contains unit tests for the PolicyCreationEventHandler.

use chrono::Utc;
use shared::hrn::Hrn;

use crate::domain::events::PolicyCreatedEvent;
use crate::domain::ids::{OrganizationId, PolicyId};
use std::str::FromStr;
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::event_handler::{MockPolicyCreationEventHandler, SimplePolicyCreationEventHandler};

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use chrono::Utc;
    use shared::hrn::{OrganizationId, HodeiPolicyId, UserId};
    use crate::domain::events::PolicyCreatedEvent;
    use crate::features::create_policy::event_handler::{MockPolicyCreationEventHandler, PolicyCreationEventHandler, SimplePolicyCreationEventHandler};

    fn create_test_event() -> PolicyCreatedEvent {
        PolicyCreatedEvent {
            policy_id: HodeiPolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            organization_id: OrganizationId::from_str("hrn:hodei:iam::system:organization/test-org").unwrap(),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_simple_event_handler_success() {
        let handler = SimplePolicyCreationEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_created(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_event_handler_success() {
        let handler = MockPolicyCreationEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_created(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_valid_event() {
        let handler = SimplePolicyCreationEventHandler::new();
        let event = PolicyCreatedEvent {
            policy_id: HodeiPolicyId::from_str("hrn:hodei:iam::system:organization/org-456/policy/policy-123").unwrap(),
            organization_id: OrganizationId::from_str("hrn:hodei:iam::system:organization/org-456").unwrap(),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/user-789").unwrap(),
            name: "Security Policy".to_string(),
            description: Some("Policy for securing resources".to_string()),
            content: r#"permit(principal, action == "read", resource)
            when { principal.clearance >= resource.classification };"#.to_string(),
            created_at: Utc::now(),
        };

        let result = handler.handle_policy_created(event).await;

        assert!(result.is_ok());
    }
}
