// crates/artifact/src/features/versioning/mod.rs

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

// Legacy modules for backward compatibility
pub mod policy;
pub mod validator;

// Re-exportar los componentes principales
pub use dto::VersioningConfig;
pub use policy::VersioningPolicy;
pub use validator::VersionValidator;

// Expose only the public parts of the feature
pub use di::VersioningDIContainer;

#[cfg(test)]
mod use_case_test;
