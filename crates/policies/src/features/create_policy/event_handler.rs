//! Event handler for policy creation events
//!
//! This module handles domain events emitted during policy creation.

use super::error::CreatePolicyError;
use crate::domain::events::PolicyCreatedEvent;
use crate::domain::ids::PolicyId;
use async_trait::async_trait;
use tracing::{error, info};

/// Trait for handling policy creation events
#[async_trait]
pub trait PolicyCreationEventHandler: Send + Sync {
    async fn handle_policy_created(&self, event: PolicyCreatedEvent) -> Result<(), CreatePolicyError>;
}

/// Simple event handler for policy creation
pub struct SimplePolicyCreationEventHandler;

impl SimplePolicyCreationEventHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyCreationEventHandler for SimplePolicyCreationEventHandler {
    async fn handle_policy_created(&self, event: PolicyCreatedEvent) -> Result<(), CreatePolicyError> {
        info!("Handling policy created event: {}", event.policy_id);

        // Here you could:
        // - Send notifications
        // - Update search indexes
        // - Trigger background jobs
        // - Send to event stream

        info!("Policy created event handled successfully: {}", event.policy_id);
        Ok(())
    }
}

/// Mock event handler for testing
pub struct MockPolicyCreationEventHandler;

impl MockPolicyCreationEventHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyCreationEventHandler for MockPolicyCreationEventHandler {
    async fn handle_policy_created(&self, event: PolicyCreatedEvent) -> Result<(), CreatePolicyError> {
        info!("Mock handling policy created event: {}", event.policy_id);
        Ok(())
    }
}
