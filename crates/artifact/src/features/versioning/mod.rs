// crates/artifact/src/features/versioning/mod.rs

pub mod error;
pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Legacy modules for backward compatibility
pub mod validator;
pub mod policy;

// Re-exportar los componentes principales
pub use validator::VersionValidator;
pub use policy::VersioningPolicy;
pub use dto::VersioningConfig;

// Expose only the public parts of the feature
pub use di::VersioningDIContainer;
pub use api::VersioningApi;

#[cfg(test)]
mod use_case_test;
#[cfg(test)]
mod api_test;