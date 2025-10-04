use crate::shared::Hrn;
use async_trait::async_trait;
use cedar_policy::{EntityTypeName, EntityUid, Policy, RestrictedExpression};
use std::str::FromStr;
use thiserror::Error;

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
    Primitive(&'static str), // e.g. "String", "Long", "Bool"
    Set(Box<AttributeType>), // e.g. Set<String>
    EntityId(&'static str),  // e.g. EntityId<Principal> (pass the entity type name)
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
    /// Devuelve el nombre del 'servicio' que actúa como espacio de nombres.
    /// Ejemplo: "IAM", "Billing", "S3".
    fn service_name() -> &'static str;

    /// Devuelve el nombre local del tipo de recurso.
    /// Ejemplo: "User", "Group", "Bucket".
    fn resource_type_name() -> &'static str;

    /// **Método de conveniencia (con implementación por defecto).**
    /// Construye el `EntityTypeName` completo para Cedar a partir de las partes.
    fn cedar_entity_type_name() -> EntityTypeName {
        let namespace = Hrn::to_pascal_case(Self::service_name());
        let type_str = format!("{}::{}", namespace, Self::resource_type_name());
        EntityTypeName::from_str(&type_str)
            .expect("Failed to create EntityTypeName from service and resource type")
    }

    /// DEPRECATED: Use `cedar_entity_type_name()` instead.
    /// Mantener por compatibilidad temporal.
    fn entity_type_name() -> &'static str {
        // Fallback para compatibilidad: usa resource_type_name
        Self::resource_type_name()
    }

    /// Whether this entity type is a Principal in Cedar terms
    fn is_principal_type() -> bool {
        false
    }

    /// Optional: declare attributes in a typed fashion
    /// Default: empty, but recommended to provide for typed schema generation
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        Vec::new()
    }

    /// Optional: declare conceptual parent types (for membership semantics)
    /// Default: empty; membership will be modeled at data level via parents()
    fn cedar_parents_types() -> Vec<&'static str> {
        Vec::new()
    }
}

pub trait HodeiEntity {
    fn hrn(&self) -> &Hrn;
    fn attributes(&self) -> std::collections::HashMap<String, RestrictedExpression>;
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
    fn euid(&self) -> EntityUid {
        self.hrn().to_euid()
    }
}

///A marker trait for entities that can act as 'principals'.
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// A marker trait for entities that can act as 'resources'.
pub trait Resource: HodeiEntity + HodeiEntityType {}

/// Define an action that can be registered in the policy engine.
pub trait ActionTrait {
    /// The unique identifier of the action.
    fn name() -> &'static str;

    /// Define which types of Principal and Resource this action applies to.
    /// This will be used to generate the Cedar schema.
    fn applies_to() -> (EntityTypeName, EntityTypeName);
}

#[async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError>;
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError>;
}
