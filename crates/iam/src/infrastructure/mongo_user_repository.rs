use async_trait::async_trait;
use futures_util::stream::{StreamExt, TryStreamExt};
use mongodb::{bson::doc, Collection};
use shared::UserId;

use crate::{
    application::ports::UserRepository,
    domain::User,
    error::IamError,
};

pub struct MongoUserRepository {
    collection: Collection<User>,
}

impl MongoUserRepository {
    pub fn new(collection: Collection<User>) -> Self {
        Self { collection }
    }
}

#[async_trait]
#[async_trait]
impl UserRepository for MongoUserRepository {
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, IamError> {
        let filter = doc! { "id": id.to_string() };
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, IamError> {
        let filter = doc! { "username": username };
        self.collection
            .find_one(filter)
            .await
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn save(&self, user: &User) -> Result<(), IamError> {
        let filter = doc! { "id": user.id.to_string() };
        self.collection
            .replace_one(filter, user)
            .await
            .map(|_| ())
            .map_err(|e| IamError::MongoError(e.to_string()))
    }

    async fn find_all(&self) -> Result<Vec<User>, IamError> {
        let cursor = self.collection
            .find(doc!{})
            .await
            .map_err(|e| IamError::MongoError(e.to_string()))?;

        cursor
            .map(|result| result.map_err(|e| IamError::MongoError(e.to_string())))
            .try_collect()
            .await
    }

    async fn delete(&self, id: &UserId) -> Result<(), IamError> {
        let filter = doc! { "id": id.to_string() };
        self.collection
            .delete_one(filter)
            .await
            .map(|_| ())
            .map_err(|e| IamError::MongoError(e.to_string()))
    }
}
