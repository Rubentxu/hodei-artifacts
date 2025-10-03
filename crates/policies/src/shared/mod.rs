// Facade raíz del crate policies (estructura hexagonal interna)
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-exports para tests e integración
pub use application::{AuthorizationEngine, AuthorizationRequest, EngineBuilder, PolicyStore};
pub use domain::{
    entity_utils,
    hrn::Hrn,
    ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError},
    schema_assembler::*,
};

// Re-exports de Cedar comunes en tests
pub use cedar_policy::{Context, EntityUid, Policy, PolicyId};
