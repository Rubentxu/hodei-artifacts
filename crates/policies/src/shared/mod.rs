// Facade raíz del crate policies (estructura hexagonal interna)
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-exports para tests e integración - only schema-related functionality remains
pub use application::EngineBuilder;

#[cfg(feature = "legacy_infra")]
pub use domain::{entity_utils, hrn::Hrn, schema_assembler::*};

// Core ports always available
pub use domain::ports::{
    ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource,
};

// Re-exports de Cedar comunes en tests (gated behind legacy_infra)
#[cfg(feature = "legacy_infra")]
pub use cedar_policy::{Context, EntityUid, Policy, PolicyId};
