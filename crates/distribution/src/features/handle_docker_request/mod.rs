// crates/distribution/src/features/handle_docker_request/mod.rs

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Exportar los tipos públicos necesarios
pub use dto::{
    DockerManifestV2, DockerDescriptor, DockerBlobInfo, DockerManifestRequest, DockerBlobRequest,
    DockerManifestResponse, DockerBlobResponse, DockerStartUploadResponse, DockerCompleteUploadResponse,
    DockerError, DockerErrorResponse,
};
pub use ports::{
    DockerManifestReader, DockerManifestWriter, DockerBlobReader, DockerBlobWriter,
    DockerRepositoryManager, DockerPermissionChecker,
};
pub use api::DockerRegistryApi;
pub use di::DockerRegistryDIContainer;