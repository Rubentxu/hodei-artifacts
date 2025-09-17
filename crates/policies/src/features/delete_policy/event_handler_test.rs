//! Unit tests for delete_policy event handler
//!
//! This module contains unit tests for the PolicyDeletionEventHandler.

use chrono::Utc;
use shared::hrn::Hrn;

use crate::domain::events::PolicyDeletedEvent;
use crate::domain::ids::{PolicyId, UserId};
use std::str::FromStr;
use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::event_handler::{MockPolicyDeletionEventHandler, SimplePolicyDeletionEventHandler};
use crate::features::delete_policy::ports::DeletionMode;

#[cfg(test)]
mod tests {
    fn create_test_event() -> PolicyDeletedEvent {
        PolicyDeletedEvent {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            deleted_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_simple_event_handler_success() {
        let handler = SimplePolicyDeletionEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_deleted(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_event_handler_success() {
        let handler = MockPolicyDeletionEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policy_deleted(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_soft_deletion() {
        let handler = SimplePolicyDeletionEventHandler::new();
        let event = PolicyDeletedEvent {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/soft-delete-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/admin-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            deleted_at: Utc::now(),
        };

        let result = handler.handle_policy_deleted(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_hard_deletion() {
        let handler = SimplePolicyDeletionEventHandler::new();
        let event = PolicyDeletedEvent {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/hard-delete-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/admin-user").unwrap(),
            deletion_mode: DeletionMode::Hard,
            deleted_at: Utc::now(),
        };

        let result = handler.handle_policy_deleted(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_archive_deletion() {
        let handler = SimplePolicyDeletionEventHandler::new();
        let event = PolicyDeletedEvent {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/archive-delete-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/admin-user").unwrap(),
            deletion_mode: DeletionMode::Archive,
            deleted_at: Utc::now(),
        };

        let result = handler.handle_policy_deleted(event).await;

        assert!(result.is_ok());
    }
}
