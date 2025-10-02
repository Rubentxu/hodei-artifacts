// IAM Crate
pub mod application;
pub mod domain;
pub mod features;
pub mod infrastructure;

#[cfg(test)]
pub mod test_utils;

// Re-export commonly used types
pub use domain::policy::{Policy, PolicyError, PolicyMetadata, PolicyStatus};
pub use infrastructure::errors::{IamError, ValidationError};
