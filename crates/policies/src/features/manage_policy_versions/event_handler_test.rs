//! Unit tests for manage_policy_versions event handler
//!
//! This module contains unit tests for the PolicyVersionEventHandler.

use chrono::Utc;

use crate::domain::events::PolicyVersionEvent;
use crate::domain::ids::PolicyId;
use crate::features::manage_policy_versions::event_handler::{MockPolicyVersionEventHandler, SimplePolicyVersionEventHandler};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event() -> PolicyVersionEvent {
        PolicyVersionEvent {
            policy_id: PolicyId::from("test-policy"),
            version: 2,
            event_type: "version_created".to_string(),
            from_version: None,
            cleaned_versions: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_simple_event_handler_version_created() {
        let handler = SimplePolicyVersionEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_version_event(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_event_handler_success() {
        let handler = MockPolicyVersionEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_version_event(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_version_rollback() {
        let handler = SimplePolicyVersionEventHandler::new();
        let event = PolicyVersionEvent {
            policy_id: PolicyId::from("rollback-policy"),
            version: 3,
            event_type: "version_rollback".to_string(),
            from_version: Some(5),
            cleaned_versions: None,
            created_at: Utc::now(),
        };

        let result = handler.handle_policy_version_event(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_versions_cleaned() {
        let handler = SimplePolicyVersionEventHandler::new();
        let event = PolicyVersionEvent {
            policy_id: PolicyId::from("cleanup-policy"),
            version: 0,
            event_type: "versions_cleaned".to_string(),
            from_version: None,
            cleaned_versions: Some(vec![1, 2, 3]),
            created_at: Utc::now(),
        };

        let result = handler.handle_policy_version_event(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_unknown_event_type() {
        let handler = SimplePolicyVersionEventHandler::new();
        let event = PolicyVersionEvent {
            policy_id: PolicyId::from("test-policy"),
            version: 1,
            event_type: "unknown_event".to_string(),
            from_version: None,
            cleaned_versions: None,
            created_at: Utc::now(),
        };

        let result = handler.handle_policy_version_event(event).await;

        assert!(result.is_ok());
    }
}
