// crates/distribution/src/features/handle_docker_request/di.rs

use std::sync::Arc;
use crate::features::handle_docker_request::{
    api::DockerRegistryApi,
    use_case::{
        HandleDockerGetManifestUseCase, HandleDockerPutManifestUseCase, HandleDockerHeadManifestUseCase,
        HandleDockerDeleteManifestUseCase, HandleDockerGetBlobUseCase, HandleDockerPutBlobUseCase,
        HandleDockerStartBlobUploadUseCase, HandleDockerCompleteBlobUploadUseCase,
    },
    adapter::{
        S3DockerManifestReader, S3DockerManifestWriter, S3DockerBlobReader, S3DockerBlobWriter,
        MongoDockerRepositoryManager, CedarDockerPermissionChecker,
    },
};

/// Docker Registry API dependency injection container
pub struct DockerRegistryDIContainer {
    pub api: DockerRegistryApi,
}

impl DockerRegistryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        manifest_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestReader>,
        manifest_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestWriter>,
        blob_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobReader>,
        blob_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobWriter>,
        repository_manager: Arc<dyn crate::features::handle_docker_request::ports::DockerRepositoryManager>,
        permission_checker: Arc<dyn crate::features::handle_docker_request::ports::DockerPermissionChecker>,
    ) -> Self {
        let get_manifest_use_case = Arc::new(HandleDockerGetManifestUseCase::new(
            manifest_reader.clone(),
            permission_checker.clone(),
        ));

        let put_manifest_use_case = Arc::new(HandleDockerPutManifestUseCase::new(
            manifest_writer.clone(),
            permission_checker.clone(),
        ));

        let head_manifest_use_case = Arc::new(HandleDockerHeadManifestUseCase::new(
            manifest_reader.clone(),
            permission_checker.clone(),
        ));

        let delete_manifest_use_case = Arc::new(HandleDockerDeleteManifestUseCase::new(
            manifest_writer.clone(),
            permission_checker.clone(),
        ));

        let get_blob_use_case = Arc::new(HandleDockerGetBlobUseCase::new(
            blob_reader.clone(),
            permission_checker.clone(),
        ));

        let put_blob_use_case = Arc::new(HandleDockerPutBlobUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        let start_blob_upload_use_case = Arc::new(HandleDockerStartBlobUploadUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        let complete_blob_upload_use_case = Arc::new(HandleDockerCompleteBlobUploadUseCase::new(
            blob_writer.clone(),
            permission_checker.clone(),
        ));

        let api = DockerRegistryApi::new(
            get_manifest_use_case,
            put_manifest_use_case,
            head_manifest_use_case,
            delete_manifest_use_case,
            get_blob_use_case,
            put_blob_use_case,
            start_blob_upload_use_case,
            complete_blob_upload_use_case,
        );

        Self { api }
    }

    /// Método de conveniencia para producción con S3 y MongoDB
    pub fn for_production(
        s3_client: Arc<dyn aws_sdk_s3::Client>,
        s3_bucket: String,
        mongo_client: Arc<dyn mongodb::Client>,
        mongo_database: String,
        cedar_engine: Arc<dyn cedar_policy::Engine>,
    ) -> Self {
        let manifest_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestReader> = 
            Arc::new(S3DockerManifestReader::new(s3_client.clone(), s3_bucket.clone()));
        
        let manifest_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestWriter> = 
            Arc::new(S3DockerManifestWriter::new(s3_client.clone(), s3_bucket.clone()));
        
        let blob_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobReader> = 
            Arc::new(S3DockerBlobReader::new(s3_client.clone(), s3_bucket.clone()));
        
        let blob_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobWriter> = 
            Arc::new(S3DockerBlobWriter::new(s3_client.clone(), s3_bucket.clone()));
        
        let repository_manager: Arc<dyn crate::features::handle_docker_request::ports::DockerRepositoryManager> = 
            Arc::new(MongoDockerRepositoryManager::new(mongo_client, mongo_database));
        
        let permission_checker: Arc<dyn crate::features::handle_docker_request::ports::DockerPermissionChecker> = 
            Arc::new(CedarDockerPermissionChecker::new(cedar_engine));

        Self::new(
            manifest_reader,
            manifest_writer,
            blob_reader,
            blob_writer,
            repository_manager,
            permission_checker,
        )
    }

    /// Método de conveniencia para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use crate::features::handle_docker_request::ports::{
            MockDockerManifestReader, MockDockerManifestWriter, MockDockerBlobReader, 
            MockDockerBlobWriter, MockDockerRepositoryManager, MockDockerPermissionChecker,
        };

        let manifest_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestReader> = 
            Arc::new(MockDockerManifestReader::new());
        
        let manifest_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerManifestWriter> = 
            Arc::new(MockDockerManifestWriter::new());
        
        let blob_reader: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobReader> = 
            Arc::new(MockDockerBlobReader::new());
        
        let blob_writer: Arc<dyn crate::features::handle_docker_request::ports::DockerBlobWriter> = 
            Arc::new(MockDockerBlobWriter::new());
        
        let repository_manager: Arc<dyn crate::features::handle_docker_request::ports::DockerRepositoryManager> = 
            Arc::new(MockDockerRepositoryManager::new());
        
        let permission_checker: Arc<dyn crate::features::handle_docker_request::ports::DockerPermissionChecker> = 
            Arc::new(MockDockerPermissionChecker::new());

        Self::new(
            manifest_reader,
            manifest_writer,
            blob_reader,
            blob_writer,
            repository_manager,
            permission_checker,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_registry_di_container_creation() {
        let container = DockerRegistryDIContainer::for_testing();
        
        // Test that the API is properly initialized
        assert!(std::ptr::eq(
            &container.api.get_manifest_use_case,
            &container.api.get_manifest_use_case
        ));
    }

    #[test]
    fn test_docker_registry_di_container_with_custom_implementations() {
        use crate::features::handle_docker_request::ports::{
            MockDockerManifestReader, MockDockerManifestWriter, MockDockerBlobReader, 
            MockDockerBlobWriter, MockDockerRepositoryManager, MockDockerPermissionChecker,
        };

        let manifest_reader = Arc::new(MockDockerManifestReader::new());
        let manifest_writer = Arc::new(MockDockerManifestWriter::new());
        let blob_reader = Arc::new(MockDockerBlobReader::new());
        let blob_writer = Arc::new(MockDockerBlobWriter::new());
        let repository_manager = Arc::new(MockDockerRepositoryManager::new());
        let permission_checker = Arc::new(MockDockerPermissionChecker::new());

        let container = DockerRegistryDIContainer::new(
            manifest_reader.clone(),
            manifest_writer.clone(),
            blob_reader.clone(),
            blob_writer.clone(),
            repository_manager.clone(),
            permission_checker.clone(),
        );

        // Verify that the container is properly initialized
        assert!(std::ptr::eq(
            &container.api.get_manifest_use_case,
            &container.api.get_manifest_use_case
        ));
    }
}