use async_trait::async_trait;
use mongodb::{bson::doc, Collection};
use crate::domain::PolicyId;
use futures_util::stream::{StreamExt, TryStreamExt};

use crate::{
    application::ports::PolicyRepository,
    domain::Policy,
    error::IamError,
};

pub struct MongoPolicyRepository {
    collection: Collection<Policy>,
}

impl MongoPolicyRepository {
    pub fn new(collection: Collection<Policy>) -> Self {
        Self { collection }
    }
}

#[async_trait]
impl PolicyRepository for MongoPolicyRepository {
    async fn find_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "id": id.to_string() };
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn save(&self, policy: &Policy) -> Result<(), IamError> {
        let filter = doc! { "id": policy.id.to_string() };
        self.collection
            .replace_one(filter, policy)
            .await
            .map(|_| ())
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn delete(&self, id: &PolicyId) -> Result<(), IamError> {
        let filter = doc! { "id": id.to_string() };
        self.collection
            .delete_one(filter)
            .await
            .map(|_| ())
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn find_all(&self) -> Result<Vec<Policy>, IamError> {
        let cursor = self.collection
            .find(doc!{})
            .await
            .map_err(|e| IamError::MongoError(e.to_string()))?;

        cursor
            .map(|result| result.map_err(|e| IamError::MongoError(e.to_string())))
            .try_collect()
            .await
    }
}