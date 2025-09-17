//! Unit tests for list_policies event handler
//!
//! This module contains unit tests for the PolicyListingEventHandler.

use chrono::Utc;

use crate::domain::events::PoliciesListedEvent;
use crate::domain::ids::UserId;
use crate::features::list_policies::event_handler::{MockPolicyListingEventHandler, SimplePolicyListingEventHandler};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event() -> PoliciesListedEvent {
        PoliciesListedEvent {
            listed_by: UserId::from("test-user"),
            result_count: 5,
            query_filters: std::collections::HashMap::new(),
            listed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_simple_event_handler_success() {
        let handler = SimplePolicyListingEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policies_listed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_event_handler_success() {
        let handler = MockPolicyListingEventHandler::new();
        let event = create_test_event();

        let result = handler.handle_policies_listed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_many_results() {
        let handler = SimplePolicyListingEventHandler::new();
        let event = PoliciesListedEvent {
            listed_by: UserId::from("admin-user"),
            result_count: 100,
            query_filters: vec![("status".to_string(), "active".to_string())].into_iter().collect(),
            listed_at: Utc::now(),
        };

        let result = handler.handle_policies_listed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_with_filters() {
        let handler = SimplePolicyListingEventHandler::new();
        let mut query_filters = std::collections::HashMap::new();
        query_filters.insert("organization_id".to_string(), "org-123".to_string());
        query_filters.insert("status".to_string(), "active".to_string());

        let event = PoliciesListedEvent {
            listed_by: UserId::from("analyst-user"),
            result_count: 25,
            query_filters,
            listed_at: Utc::now(),
        };

        let result = handler.handle_policies_listed(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_handler_zero_results() {
        let handler = SimplePolicyListingEventHandler::new();
        let event = PoliciesListedEvent {
            listed_by: UserId::from("test-user"),
            result_count: 0,
            query_filters: std::collections::HashMap::new(),
            listed_at: Utc::now(),
        };

        let result = handler.handle_policies_listed(event).await;

        assert!(result.is_ok());
    }
}
