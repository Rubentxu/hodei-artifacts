// crates/distribution/src/domain/npm/mod.rs

pub mod npm_handler;
pub mod npm_metadata;
pub mod npm_paths;

// Re-exportar componentes npm
pub use npm_handler::NpmFormatHandler;
pub use npm_metadata::{NpmPackageMetadata, NpmMetadataGenerator};
pub use npm_paths::{NpmPathInfo, NpmPathParser};