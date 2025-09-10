// crates/distribution/src/features/generate_docker_manifest/mod.rs

//! Feature de generación de manifests Docker
//! 
//! Esta feature proporciona funcionalidad para generar manifests Docker dinámicamente
//! a partir de las capas disponibles en el repositorio, con soporte para diferentes
//! tipos de manifests (V2.1, V2.2, manifest lists) y formatos OCI.

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Re-exportar los tipos públicos principales
pub use dto::{
    GenerateDockerManifestRequest, GenerateDockerManifestResponse, DockerManifestDto,
    DockerDescriptorDto, DockerManifestListDto, DockerPlatformDto,
};
pub use api::GenerateDockerManifestApi;
pub use di::GenerateDockerManifestDIContainer;
pub use use_case::GenerateDockerManifestUseCase;

// Re-exportar tipos de errores
pub use ports::{
    DockerManifestGeneratorError, DockerLayerListerError, DockerManifestCacheError,
};