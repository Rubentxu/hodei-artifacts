pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
pub mod use_case_chunks;
pub mod upload_progress;

#[cfg(test)]
pub mod test_adapter;
#[cfg(test)]
mod use_case_test;
#[cfg(test)]
mod api_test;
#[cfg(test)]
mod use_case_chunks_test;

// Expose only the public parts of the feature.
pub use di::UploadArtifactDIContainer;
pub use dto::{UploadArtifactCommand, UploadArtifactResponse};
pub use error::UploadArtifactError;