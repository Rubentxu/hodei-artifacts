// crates/distribution/src/domain/docker/mod.rs

pub mod docker_handler;
pub mod docker_manifest;
pub mod docker_paths;

// Re-exportar componentes Docker
pub use docker_handler::DockerFormatHandler;
pub use docker_manifest::{DockerManifest, DockerManifestGenerator, DockerManifestV2, DockerManifestList};
pub use docker_paths::{DockerPathInfo, DockerPathParser};