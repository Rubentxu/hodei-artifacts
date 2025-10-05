// Legacy Surreal in-memory storage (deprecated). Compile only when explicitly enabling `legacy_infra`.
#[cfg(feature = "legacy_infra")]
pub mod mem_storage;

#[cfg(feature = "legacy_infra")]
pub use mem_storage::SurrealMemStorage;

#[cfg(feature = "embedded")]
pub mod embedded_storage;

#[cfg(feature = "embedded")]
pub use embedded_storage::SurrealEmbeddedStorage;
