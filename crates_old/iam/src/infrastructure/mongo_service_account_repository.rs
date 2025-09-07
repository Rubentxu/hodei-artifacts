#![allow(dead_code, unused_imports)]
use async_trait::async_trait;
use mongodb::Collection;

use shared::ServiceAccountId;
use crate::application::ports::ServiceAccountRepository;
use crate::domain::ServiceAccount;
use crate::error::IamError;

pub struct MongoServiceAccountRepository {
    collection: Collection<ServiceAccount>,
}

impl MongoServiceAccountRepository {
    pub fn new(collection: Collection<ServiceAccount>) -> Self {
        Self { collection }
    }
}

#[async_trait]
impl ServiceAccountRepository for MongoServiceAccountRepository {
    async fn find_by_id(&self, _id: &ServiceAccountId) -> Result<Option<ServiceAccount>, IamError> {
        // TODO: Implementar la lógica de búsqueda en MongoDB
        unimplemented!()
    }

    async fn save(&self, _sa: &ServiceAccount) -> Result<(), IamError> {
        // TODO: Implementar la lógica de guardado en MongoDB
        unimplemented!()
    }
}
