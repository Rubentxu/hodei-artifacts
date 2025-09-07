//! Slice de descarga de artifacts.
//!
//! Implementa la funcionalidad de descarga de artifacts tanto mediante streaming directo
//! como a través de URLs presignadas para optimizar el rendimiento.

pub mod query;
pub mod logic;
pub mod handler;
pub mod instrumentation;

pub use query::{GetArtifactQuery, GetArtifactResponse, DownloadMethod};
pub use logic::handle_get_artifact;
pub use handler::handle;
