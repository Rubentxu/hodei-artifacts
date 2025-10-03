// Local modules in shared/domain
pub mod entity_utils;
pub mod error;
pub mod hrn;
pub mod ports;
pub mod schema_assembler;

// Convenience re-exports for external use
pub use error::HodeiPoliciesError;
pub use hrn::Hrn;
pub use ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError};