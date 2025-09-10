// crates/distribution/src/features/handle_npm_request/di.rs

//! Contenedor de inyección de dependencias para el feature Handle NPM Request
//! 
//! Proporciona configuración flexible de dependencias para diferentes entornos.

use std::sync::Arc;
use super::ports::{
    NpmPackageReader, NpmPackageWriter, NpmRepositoryManager, NpmPermissionChecker,
};
use super::use_case::{
    HandleNpmGetPackageUseCase, HandleNpmPutPackageUseCase, HandleNpmHeadPackageUseCase,
    HandleNpmGetPackageJsonUseCase, HandleNpmGetRepositoryInfoUseCase, HandleNpmSearchUseCase,
    HandleNpmGetDistTagsUseCase, HandleNpmUpdateDistTagsUseCase,
};
use super::api::NpmRequestHandler;

/// Contenedor de DI para el feature Handle NPM Request
pub struct HandleNpmRequestDIContainer {
    pub get_package_use_case: Arc<HandleNpmGetPackageUseCase>,
    pub put_package_use_case: Arc<HandleNpmPutPackageUseCase>,
    pub head_package_use_case: Arc<HandleNpmHeadPackageUseCase>,
    pub get_package_json_use_case: Arc<HandleNpmGetPackageJsonUseCase>,
    pub get_repository_info_use_case: Arc<HandleNpmGetRepositoryInfoUseCase>,
    pub search_use_case: Arc<HandleNpmSearchUseCase>,
    pub get_dist_tags_use_case: Arc<HandleNpmGetDistTagsUseCase>,
    pub update_dist_tags_use_case: Arc<HandleNpmUpdateDistTagsUseCase>,
    pub request_handler: NpmRequestHandler,
}

impl HandleNpmRequestDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        package_reader: Arc<dyn NpmPackageReader>,
        package_writer: Arc<dyn NpmPackageWriter>,
        repository_manager: Arc<dyn NpmRepositoryManager>,
        permission_checker: Arc<dyn NpmPermissionChecker>,
    ) -> Self {
        let get_package_use_case = Arc::new(HandleNpmGetPackageUseCase::new(
            package_reader.clone(),
            permission_checker.clone(),
        ));
        
        let put_package_use_case = Arc::new(HandleNpmPutPackageUseCase::new(
            package_writer.clone(),
            repository_manager.clone(),
            permission_checker.clone(),
        ));
        
        let head_package_use_case = Arc::new(HandleNpmHeadPackageUseCase::new(
            package_reader.clone(),
            permission_checker.clone(),
        ));
        
        let get_package_json_use_case = Arc::new(HandleNpmGetPackageJsonUseCase::new(
            package_reader.clone(),
            permission_checker.clone(),
        ));
        
        let get_repository_info_use_case = Arc::new(HandleNpmGetRepositoryInfoUseCase::new(
            repository_manager.clone(),
            permission_checker.clone(),
        ));
        
        let search_use_case = Arc::new(HandleNpmSearchUseCase::new(
            package_reader.clone(),
            permission_checker.clone(),
        ));
        
        let get_dist_tags_use_case = Arc::new(HandleNpmGetDistTagsUseCase::new(
            package_reader.clone(),
            permission_checker.clone(),
        ));
        
        let update_dist_tags_use_case = Arc::new(HandleNpmUpdateDistTagsUseCase::new(
            package_writer.clone(),
            permission_checker.clone(),
        ));
        
        let request_handler = NpmRequestHandler::new(
            get_package_use_case.clone(),
            put_package_use_case.clone(),
            head_package_use_case.clone(),
            get_package_json_use_case.clone(),
            get_repository_info_use_case.clone(),
            search_use_case.clone(),
            get_dist_tags_use_case.clone(),
            update_dist_tags_use_case.clone(),
        );
        
        Self {
            get_package_use_case,
            put_package_use_case,
            head_package_use_case,
            get_package_json_use_case,
            get_repository_info_use_case,
            search_use_case,
            get_dist_tags_use_case,
            update_dist_tags_use_case,
            request_handler,
        }
    }
    
    /// Método de conveniencia para producción con S3, MongoDB y Cedar
    pub fn for_production(
        s3_client: Arc<dyn super::adapter::S3Client>,
        mongo_client: Arc<dyn super::adapter::MongoClient>,
        cedar_engine: Arc<dyn super::adapter::CedarEngine>,
        bucket_name: String,
        database_name: String,
        base_path: String,
    ) -> Self {
        let package_reader: Arc<dyn NpmPackageReader> = Arc::new(
            super::adapter::S3NpmPackageReader::new(
                s3_client.clone(),
                bucket_name.clone(),
                base_path.clone(),
            )
        );
        
        let package_writer: Arc<dyn NpmPackageWriter> = Arc::new(
            super::adapter::S3NpmPackageWriter::new(
                s3_client,
                bucket_name,
                base_path,
            )
        );
        
        let repository_manager: Arc<dyn NpmRepositoryManager> = Arc::new(
            super::adapter::MongoNpmRepositoryManager::new(
                mongo_client,
                database_name,
            )
        );
        
        let permission_checker: Arc<dyn NpmPermissionChecker> = Arc::new(
            super::adapter::CedarNpmPermissionChecker::new(cedar_engine)
        );
        
        Self::new(
            package_reader,
            package_writer,
            repository_manager,
            permission_checker,
        )
    }
    
    /// Método de conveniencia para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::{MockS3Client, MockMongoClient, MockCedarEngine};
        
        let s3_client: Arc<dyn super::adapter::S3Client> = Arc::new(MockS3Client::new());
        let mongo_client: Arc<dyn super::adapter::MongoClient> = Arc::new(MockMongoClient::new());
        let cedar_engine: Arc<dyn super::adapter::CedarEngine> = Arc::new(MockCedarEngine::new());
        
        Self::for_production(
            s3_client,
            mongo_client,
            cedar_engine,
            "test-bucket".to_string(),
            "test-database".to_string(),
            "test-artifacts".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::{NpmPackageName, NpmVersion};
    
    #[tokio::test]
    async fn test_di_container_creation() {
        let container = HandleNpmRequestDIContainer::for_testing();
        
        // Verificar que todos los use cases están inicializados
        assert!(Arc::strong_count(&container.get_package_use_case) > 0);
        assert!(Arc::strong_count(&container.put_package_use_case) > 0);
        assert!(Arc::strong_count(&container.head_package_use_case) > 0);
        assert!(Arc::strong_count(&container.get_package_json_use_case) > 0);
        assert!(Arc::strong_count(&container.get_repository_info_use_case) > 0);
        assert!(Arc::strong_count(&container.search_use_case) > 0);
        assert!(Arc::strong_count(&container.get_dist_tags_use_case) > 0);
        assert!(Arc::strong_count(&container.update_dist_tags_use_case) > 0);
    }
    
    #[tokio::test]
    async fn test_request_handler_integration() {
        let container = HandleNpmRequestDIContainer::for_testing();
        
        // Crear un paquete de prueba
        let package_name = NpmPackageName::new("test-package").unwrap();
        let version = NpmVersion::new("1.0.0").unwrap();
        
        // Verificar que el request handler puede procesar solicitudes
        let handler = &container.request_handler;
        
        // Esto debería funcionar sin errores (aunque devuelva un error de paquete no encontrado)
        let result = handler.get_package(
            package_name.clone(),
            version.clone(),
            "test-repo".to_string(),
            "test-user".to_string(),
        ).await;
        
        // Debería fallar con PackageNotFound ya que no hemos creado el paquete
        assert!(result.is_err());
        match result {
            Err(e) => {
                let error_msg = format!("{}", e);
                assert!(error_msg.contains("not found") || error_msg.contains("PackageNotFound"));
            }
            Ok(_) => panic!("Expected error but got success"),
        }
    }
}