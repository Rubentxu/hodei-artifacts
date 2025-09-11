// crates/artifact/src/features/versioning/mod.rs

pub mod validator;
pub mod policy;
pub mod error;
pub mod dto;

// Re-exportar los componentes principales
pub use validator::VersionValidator;
pub use policy::VersioningPolicy;
pub use dto::VersioningConfig;