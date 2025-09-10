// crates/distribution/src/lib.rs

pub mod domain;
pub mod application;
pub mod features;
pub mod infrastructure;

// Re-exportar componentes principales
pub use domain::{
    FormatHandler, FormatRequest, FormatResponse,
    DistributionError, DistributionResult,
    maven::MavenFormatHandler,
    npm::NpmFormatHandler,
};

pub use application::FormatHandlerRegistry;
