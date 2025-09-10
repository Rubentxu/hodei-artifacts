// crates/distribution/src/domain/mod.rs

pub mod format_handler;
pub mod maven;
pub mod npm;
pub mod docker;
pub mod error;

// Re-exportar dominios específicos
pub use format_handler::{FormatHandler, FormatHandlerResult, FormatRequest, FormatResponse};
pub use error::{DistributionError, FormatError};