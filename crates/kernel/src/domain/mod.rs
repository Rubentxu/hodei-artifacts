#![allow(clippy::module_name_repetitions)]
//! Dominio compartido (Shared Kernel) para el ecosistema Hodei.
//!
//! Este módulo expone únicamente los elementos de lenguaje
//! verdaderamente transversales (HRN + metadatos de entidades + traits
//! para el motor de políticas). Cualquier crate que necesite
//! describir entidades, acciones o construir UIDs de Cedar debe
//! depender de este módulo en lugar de acoplarse a un bounded context
//! concreto (p.ej. `policies`, `hodei-iam`, etc.).
//!
//! Principios:
//! - No incluir lógica de negocio específica.
//! - Solo tipos estables y abstracciones (HRN, traits de entidades, storage de políticas).
//! - Evitar introducir dependencias cíclicas (este módulo debe permanecer "en la base").
//!
//! Estructura:
//! - `hrn`: Representa el identificador global de recursos (Hrn).
//! - `entity`: Traits y tipos para describir entidades, acciones y almacenamiento de políticas.
//! - `value_objects`: Value Objects tipados del dominio (ServiceName, ResourceTypeName, etc.)
//! - `attributes`: Tipos agnósticos para representar valores de atributos
//!
//! Re-exports clave para ergonomía:
//! - `Hrn`
//! - `HodeiEntityType`, `HodeiEntity`, `Principal`, `Resource`
//! - `ActionTrait`, `AttributeType`
//! - `PolicyStorage`, `PolicyStorageError`
//! - `ServiceName`, `ResourceTypeName`, `AttributeName`, `ValidationError`
//! - `AttributeValue`

pub mod attributes;
pub mod entity;
pub mod hrn;
pub mod value_objects;

#[cfg(test)]
mod hrn_test;

// Re-export de tipos fundamentales para uso directo por consumidores.
pub use entity::{
    ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStorageError,
    Principal, Resource,
};
pub use hrn::Hrn;

// Re-export de Value Objects para uso ergonómico
pub use value_objects::{AttributeName, ResourceTypeName, ServiceName, ValidationError};

// Re-export de tipos de atributos agnósticos
pub use attributes::AttributeValue;
