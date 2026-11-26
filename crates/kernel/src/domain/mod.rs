#![allow(clippy::module_name_repetitions)]
//! Shared domain (Shared Kernel) for the Hodei ecosystem.
//!
//! This module exposes only the truly cross-cutting language
//! elements (HRN + entity metadata + traits
//! for the policy engine). Any crate that needs to
//! describe entities, actions or build Cedar UIDs must
//! depend on this module instead of coupling to a specific
//! bounded context (e.g. `policies`, `hodei-iam`, etc.).
//!
//! Principles:
//! - Do not include specific business logic.
//! - Only stable types and abstractions (HRN, entity traits, policy storage).
//! - Avoid introducing cyclic dependencies (this module must remain "at the base").
//!
//! Structure:
//! - `hrn`: Represents the global resource identifier (Hrn).
//! - `entity`: Traits and types to describe entities, actions and policy storage.
//! - `value_objects`: Typed Value Objects of the domain (ServiceName, ResourceTypeName, etc.)
//! - `attributes`: Agnostic types to represent attribute values
//!
//! Key re-exports for ergonomics:
//! - `Hrn`
//! - `HodeiEntityType`, `HodeiEntity`, `Principal`, `Resource`
//! - `ActionTrait`, `AttributeType`
//! - `PolicyStorage`, `PolicyStorageError`
//! - `ServiceName`, `ResourceTypeName`, `AttributeName`, `ValidationError`
//! - `AttributeValue`

pub mod attributes;
pub mod entity;
pub mod hrn;
pub mod hrn_migration;
pub mod policy;
pub mod value_objects;

#[cfg(test)]
mod hrn_test;

// Re-export of fundamental types for direct use by consumers.
pub use entity::{
    ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStorageError,
    Principal, Resource,
};
pub use hrn::Hrn;
pub use hrn_migration::{HrnMigrationError, HrnSource, MigratedHrn};

// Re-export of Value Objects for ergonomic use
pub use value_objects::{AttributeName, ResourceTypeName, ServiceName, ValidationError};

// Re-export of agnostic attribute types
pub use attributes::AttributeValue;

// Re-export of agnostic policy types
pub use policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
