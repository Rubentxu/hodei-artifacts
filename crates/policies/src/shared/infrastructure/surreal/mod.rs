pub mod mem_storage;

pub use mem_storage::SurrealMemStorage;

#[cfg(feature = "embedded")]
pub mod embedded_storage;

#[cfg(feature = "embedded")]
pub use embedded_storage::SurrealEmbeddedStorage;
