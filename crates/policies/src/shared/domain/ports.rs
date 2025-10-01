
use async_trait::async_trait;
use cedar_policy::{
    EntityId, EntityTypeName, EntityUid, Policy, RestrictedExpression,
};
use std::str::FromStr;
use thiserror::Error;
use crate::shared::Hrn;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("Policy parsing error: {0}")]
    ParsingError(String),
}

/// Attribute types to describe Cedar schema attributes in a typed way
#[derive(Debug, Clone)]
pub enum AttributeType {
    Primitive(&'static str),            // e.g. "String", "Long", "Bool"
    Set(Box<AttributeType>),           // e.g. Set<String>
    EntityId(&'static str),            // e.g. EntityId<Principal> (pass the entity type name)
}

impl AttributeType {
    pub fn to_cedar_decl(&self) -> String {
        match self {
            AttributeType::Primitive(name) => name.to_string(),
            AttributeType::Set(inner) => format!("Set<{}>", inner.to_cedar_decl()),
            AttributeType::EntityId(entity_ty) => format!("EntityId<{}>", entity_ty),
        }
    }
}

/// Type-level metadata for building Cedar schema fragments from Rust types
pub trait HodeiEntityType {
    /// The Cedar entity type name (e.g., "User", "Group")
    fn entity_type_name() -> &'static str;

    /// Whether this entity type is a Principal in Cedar terms
    fn is_principal_type() -> bool { false }

    /// Optional: declare attributes in a typed fashion
    /// Default: empty, but recommended to provide for typed schema generation
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> { Vec::new() }

    /// Optional: declare conceptual parent types (for membership semantics)
    /// Default: empty; membership will be modeled at data level via parents()
    fn cedar_parents_types() -> Vec<&'static str> { Vec::new() }
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
        let type_name: EntityTypeName =
            EntityTypeName::from_str(hrn.resource_type.as_str()).unwrap();
        EntityUid::from_type_name_and_id(type_name, eid)
    }
}

#[async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError>;
}
