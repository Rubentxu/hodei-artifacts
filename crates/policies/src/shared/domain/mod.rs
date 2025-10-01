// Local modules in shared/domain
pub mod actions;
pub mod entity_utils;
pub mod hrn;
pub mod ports;
pub mod principals;
pub mod schema_assembler;

// Convenience re-exports for external use
pub use hrn::Hrn;
pub use ports::{HodeiEntity, HodeiEntityType, PolicyStorage, StorageError, AttributeType};
