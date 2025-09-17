//! Unit tests for get_policy event handler
//!
//! This module contains unit tests for the PolicyRetrievalEventHandler.

use chrono::Utc;

use crate::domain::events::PolicyAccessedEvent;
use crate::domain::ids::{PolicyId, UserId};
use crate::features::get_policy::event_handler::{MockPolicyRetrievalEventHandler, SimplePolicyRetrievalEventHandler};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event() -> PolicyAccessedEvent {
        PolicyAccessedEvent {
            policy_id: PolicyId::from("test-policy"),
            accessed_by: UserId::from("test-user"),
            access_type: "read".to_string(),
            accessed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_simple_event_handler_success() {
        let handler = SimplePolicyRetrievalEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_accessed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_event_handler_success() {
        let handler = MockPolicyRetrievalEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_accessed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_read_access() {
        let handler = SimplePolicyRetrievalEventHandler::new();
        let event = PolicyAccessedEvent {
            policy_id: PolicyId::from("security-policy"),
            accessed_by: UserId::from("analyst-user"),
            access_type: "read".to_string(),
            accessed_at: Utc::now(),
        };

        let result = handler.handle_policy_accessed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_detailed_access() {
        let handler = SimplePolicyRetrievalEventHandler::new();
        let event = PolicyAccessedEvent {
            policy_id: PolicyId::from("compliance-policy"),
            accessed_by: UserId::from("auditor-user"),
            access_type: "detailed_read".to_string(),
            accessed_at: Utc::now(),
        };

        let result = handler.handle_policy_accessed(event).await;

        assert!(result.is_ok());
    }
}
