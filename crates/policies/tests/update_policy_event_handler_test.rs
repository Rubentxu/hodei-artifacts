//! Clean unit tests for update_policy event handler
//!
//! This module contains clean unit tests for the PolicyUpdateEventHandler.

use chrono::Utc;
use std::str::FromStr;

use policies::domain::events::PolicyUpdatedEvent;
use policies::domain::ids::PolicyId;
use policies::features::update_policy::event_handler::{MockPolicyUpdateEventHandler, SimplePolicyUpdateEventHandler, PolicyUpdateEventHandler};
use shared::hrn::UserId;

/// Create a test event for use in tests
fn create_test_event() -> PolicyUpdatedEvent {
    PolicyUpdatedEvent {
        policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
        updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        changes: vec!["Policy name updated".to_string(), "Policy content updated".to_string()],
        new_version: 2,
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_simple_event_handler_success() {
    let handler = SimplePolicyUpdateEventHandler::new();
    let event = create_test_event();

    let result = handler.handle_policy_updated(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_event_handler_success() {
    let handler = MockPolicyUpdateEventHandler::new();
    let event = create_test_event();

    let result = handler.handle_policy_updated(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_handler_with_changes() {
    let handler = SimplePolicyUpdateEventHandler::new();
    let event = PolicyUpdatedEvent {
        policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/security-policy").unwrap(),
        updated_by: UserId::from_str("hrn:hodei:iam::system:user/admin-user").unwrap(),
        changes: vec![
            "Policy content updated".to_string(),
            "Policy version incremented".to_string(),
        ],
        new_version: 5,
        updated_at: Utc::now(),
    };

    let result = handler.handle_policy_updated(event).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_handler_empty_changes() {
    let handler = SimplePolicyUpdateEventHandler::new();
    let event = PolicyUpdatedEvent {
        policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
        updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        changes: vec![],
        new_version: 1,
        updated_at: Utc::now(),
    };

    let result = handler.handle_policy_updated(event).await;

    assert!(result.is_ok());
}