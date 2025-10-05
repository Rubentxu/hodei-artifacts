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
//! - Evitar introducir dependencias cíclicas (este módulo debe permanecer “en la base”).
//!
//! Estructura:
//! - `hrn`: Representa el identificador global de recursos (Hrn).
//! - `entity`: Traits y tipos para describir entidades, acciones y almacenamiento de políticas.
//!
//! Re-exports clave para ergonomía:
//! - `Hrn`
//! - `HodeiEntityType`, `HodeiEntity`, `Principal`, `Resource`
//! - `ActionTrait`, `AttributeType`
//! - `PolicyStorage`, `PolicyStorageError`

pub mod hrn;
pub mod entity;

// Re-export de tipos fundamentales para uso directo por consumidores.
pub use hrn::Hrn;
pub use entity::{
    AttributeType,
    HodeiEntityType,
    HodeiEntity,
    Principal,
    Resource,
    ActionTrait,
    PolicyStorage,
    PolicyStorageError,
};
