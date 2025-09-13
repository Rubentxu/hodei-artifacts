// crates/iam/src/infrastructure/events/policy_event_publisher.rs

use crate::domain::policy::Policy;
use crate::features::create_policy::ports::PolicyEventPublisher;
use crate::features::delete_policy::ports::PolicyDeleteEventPublisher;
use crate::features::update_policy::ports::PolicyUpdateEventPublisher;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;

/// Simple event publisher implementation
/// In a real implementation, this would integrate with a message bus like RabbitMQ or Kafka
pub struct SimplePolicyEventPublisher;

impl SimplePolicyEventPublisher {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimplePolicyEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyEventPublisher for SimplePolicyEventPublisher {
    async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError> {
        // For now, just log the event
        // In a real implementation, this would publish to a message bus
        tracing::info!(
            "Policy created event: id={}, name={}, version={}",
            policy.id.to_string(),
            policy.name,
            policy.metadata.version
        );
        Ok(())
    }
}

#[async_trait]
impl PolicyDeleteEventPublisher for SimplePolicyEventPublisher {
    async fn publish_policy_deleted(&self, policy: &Policy) -> Result<(), IamError> {
        // For now, just log the event
        // In a real implementation, this would publish to a message bus
        tracing::info!(
            "Policy deleted event: id={}, name={}, version={}",
            policy.id.to_string(),
            policy.name,
            policy.metadata.version
        );
        Ok(())
    }
}

#[async_trait]
impl PolicyUpdateEventPublisher for SimplePolicyEventPublisher {
    async fn publish_policy_updated(&self, old_policy: &Policy, new_policy: &Policy) -> Result<(), IamError> {
        // For now, just log the event
        // In a real implementation, this would publish to a message bus
        tracing::info!(
            "Policy updated event: id={}, name={}, old_version={}, new_version={}",
            new_policy.id.to_string(),
            new_policy.name,
            old_policy.metadata.version,
            new_policy.metadata.version
        );
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::{PolicyMetadata, PolicyStatus};
    use shared::hrn::{Hrn, PolicyId};
    use time::OffsetDateTime;

    fn create_test_policy() -> Policy {
        let policy_id = PolicyId(Hrn::new("hrn:hodei:iam:global:policy/test").expect("Valid HRN"));
        Policy {
            id: policy_id,
            name: "Test Policy".to_string(),
            description: None,
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Draft,
            metadata: PolicyMetadata {
                created_at: OffsetDateTime::now_utc(),
                created_by: "user_123".to_string(),
                updated_at: OffsetDateTime::now_utc(),
                updated_by: "user_123".to_string(),
                version: 1,
                tags: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn test_publish_policy_created() {
        let publisher = SimplePolicyEventPublisher::new();
        let policy = create_test_policy();

        let result = publisher.publish_policy_created(&policy).await;
        assert!(result.is_ok());
    }


}