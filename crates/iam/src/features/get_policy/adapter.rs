// crates/iam/src/features/get_policy/adapter.rs

use crate::domain::policy::Policy;
use crate::features::get_policy::ports::PolicyReader;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use mongodb::{bson::doc, Collection, Database};
use cedar_policy::PolicyId;
use std::sync::Arc;

/// Adapter that implements PolicyReader using MongoDB directly
pub struct MongoPolicyReaderAdapter {
    collection: Collection<Policy>,
}

impl MongoPolicyReaderAdapter {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            collection: database.collection::<Policy>("policies"),
        }
    }
}

#[async_trait]
impl PolicyReader for MongoPolicyReaderAdapter {
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "_id": id.to_string() };
        
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::DatabaseError(format!("Failed to get policy: {}", e)))
    }
}