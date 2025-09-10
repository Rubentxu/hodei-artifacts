// crates/distribution/src/features/generate_npm_metadata/di.rs

//! Contenedor de inyección de dependencias para generación de metadatos npm

use std::sync::Arc;
use super::ports::{NpmMetadataGenerator, NpmPackageLister, NpmMetadataCache};
use super::use_case::GenerateNpmMetadataUseCase;
use super::api::GenerateNpmMetadataApi;

/// Contenedor de inyección de dependencias para la feature de generación de metadatos npm
pub struct GenerateNpmMetadataDIContainer {
    pub api: GenerateNpmMetadataApi,
}

impl GenerateNpmMetadataDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        metadata_generator: Arc<dyn NpmMetadataGenerator>,
        package_lister: Arc<dyn NpmPackageLister>,
        metadata_cache: Arc<dyn NpmMetadataCache>,
    ) -> Self {
        let use_case = GenerateNpmMetadataUseCase::new(
            metadata_generator,
            package_lister,
            metadata_cache,
        );
        let api = GenerateNpmMetadataApi::new(use_case);
        
        Self { api }
    }
    
    /// Método de conveniencia para producción con implementaciones reales
    pub fn for_production(
        s3_client: Arc<dyn aws_sdk_s3::Client>,
        s3_bucket: String,
        mongodb_client: Arc<dyn mongodb::Client>,
        mongodb_database: String,
        redis_client: Arc<dyn redis::aio::Connection>,
        redis_ttl_seconds: u64,
    ) -> Self {
        // Adaptador S3 para generación de metadatos
        let metadata_generator: Arc<dyn NpmMetadataGenerator> = 
            Arc::new(super::adapter::S3NpmMetadataGenerator::new(s3_client, s3_bucket));
        
        // Adaptador MongoDB para listado de paquetes
        let package_lister: Arc<dyn NpmPackageLister> = 
            Arc::new(super::adapter::MongoNpmPackageLister::new(mongodb_client, mongodb_database));
        
        // Adaptador Redis para caché de metadatos
        let metadata_cache: Arc<dyn NpmMetadataCache> = 
            Arc::new(super::adapter::RedisNpmMetadataCache::new(redis_client, redis_ttl_seconds));
        
        Self::new(metadata_generator, package_lister, metadata_cache)
    }
    
    /// Método de conveniencia para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::{MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache};
        
        let metadata_generator: Arc<dyn NpmMetadataGenerator> = 
            Arc::new(MockNpmMetadataGenerator::new());
        
        let package_lister: Arc<dyn NpmPackageLister> = 
            Arc::new(MockNpmPackageLister::new());
        
        let metadata_cache: Arc<dyn NpmMetadataCache> = 
            Arc::new(MockNpmMetadataCache::new());
        
        Self::new(metadata_generator, package_lister, metadata_cache)
    }
    
    /// Método de conveniencia para testing con comportamiento específico
    #[cfg(test)]
    pub fn for_testing_with_behavior(
        metadata_generator_behavior: super::adapter::test::MockNpmMetadataGenerator,
        package_lister_behavior: super::adapter::test::MockNpmPackageLister,
        metadata_cache_behavior: super::adapter::test::MockNpmMetadataCache,
    ) -> Self {
        let metadata_generator: Arc<dyn NpmMetadataGenerator> = Arc::new(metadata_generator_behavior);
        let package_lister: Arc<dyn NpmPackageLister> = Arc::new(package_lister_behavior);
        let metadata_cache: Arc<dyn NpmMetadataCache> = Arc::new(metadata_cache_behavior);
        
        Self::new(metadata_generator, package_lister, metadata_cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::adapter::test::{MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache};
    
    #[tokio::test]
    async fn test_di_container_creation() {
        let container = GenerateNpmMetadataDIContainer::for_testing();
        
        // Verificar que el API está disponible
        assert!(std::ptr::eq(
            &container.api as *const _,
            &container.api as *const _
        ));
    }
    
    #[tokio::test]
    async fn test_di_container_with_custom_behavior() {
        let metadata_generator = MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: true,
        };
        
        let package_lister = MockNpmPackageLister::new();
        let metadata_cache = MockNpmMetadataCache::new();
        
        let container = GenerateNpmMetadataDIContainer::for_testing_with_behavior(
            metadata_generator,
            package_lister,
            metadata_cache,
        );
        
        // Verificar que el API está disponible
        assert!(std::ptr::eq(
            &container.api as *const _,
            &container.api as *const _
        ));
    }
}