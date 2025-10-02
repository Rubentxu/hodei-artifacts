// crates/iam/src/features/delete_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::features::delete_policy::ports::{PolicyDeleteEventPublisher, PolicyDeleter};
use crate::infrastructure::errors::IamError;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use async_trait::async_trait;
use cedar_policy::PolicyId;
use mongodb::{Collection, Database, bson::doc};
use std::sync::Arc;

/// Adapter that implements PolicyDeleter using MongoDB directly
pub struct DeletePolicyAdapter {
    collection: Collection<Policy>,
    event_publisher: Arc<SimplePolicyEventPublisher>,
}

impl DeletePolicyAdapter {
    pub fn new(database: Arc<Database>, event_publisher: Arc<SimplePolicyEventPublisher>) -> Self {
        Self {
            collection: database.collection::<Policy>("policies"),
            event_publisher,
        }
    }
}

#[async_trait]
impl PolicyDeleter for DeletePolicyAdapter {
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "_id": id.to_string() };

        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to get policy: {}", e)))
    }

    async fn delete(&self, id: &PolicyId) -> Result<(), IamError> {
        let filter = doc! { "_id": id.to_string() };

        let result = self
            .collection
            .delete_one(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to delete policy: {}", e)))?;

        if result.deleted_count == 0 {
            return Err(IamError::PolicyNotFound(id.clone()));
        }

        Ok(())
    }
}

#[async_trait]
impl PolicyDeleteEventPublisher for DeletePolicyAdapter {
    async fn publish_policy_deleted(&self, policy: &Policy) -> Result<(), IamError> {
        self.event_publisher.publish_policy_deleted(policy).await
    }
}
