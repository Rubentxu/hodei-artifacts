use std::str::FromStr;
use crate::domain::hrn::Hrn;
use async_trait::async_trait;
use cedar_policy::{EntityUid, SchemaFragment, Policy, SchemaError, EntityId, EntityTypeName, RestrictedExpression};
use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("Policy parsing error: {0}")]
    ParsingError(String),
}

pub trait HodeiEntityType {
    fn entity_type_name() -> &'static str;
    fn partial_schema() -> Result<SchemaFragment, SchemaError>;
}

pub trait HodeiEntity {
    fn hrn(&self) -> &Hrn;
    fn attributes(&self) -> std::collections::HashMap<String, RestrictedExpression>;
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
    fn euid(&self) -> EntityUid {
        let hrn = self.hrn();
        let eid = EntityId::from_str(hrn.resource_id.as_str()).unwrap();
        let type_name: EntityTypeName = EntityTypeName::from_str(hrn.resource_type.as_str()).unwrap();
        EntityUid::from_type_name_and_id(type_name, eid)        
    }
}

#[async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError>;
}