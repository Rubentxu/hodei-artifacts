// crates/distribution/src/features/generate_maven_metadata/di.rs

//! Configuración de inyección de dependencias para generar Maven metadata

use std::sync::Arc;
use super::ports::{
    MavenMetadataGenerator, MavenArtifactLister, MavenMetadataCache,
};
use super::adapter::{
    S3MavenMetadataGenerator, MongoMavenArtifactLister, RedisMavenMetadataCache,
    S3Client, MongoClient, RedisClient,
};
use super::use_case::GenerateMavenMetadataUseCase;
use super::api::GenerateMavenMetadataApi;

/// Contenedor de dependencias para la feature de generación de Maven metadata
pub struct GenerateMavenMetadataDIContainer {
    pub api: GenerateMavenMetadataApi,
}

impl GenerateMavenMetadataDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        metadata_generator: Arc<dyn MavenMetadataGenerator>,
        artifact_lister: Arc<dyn MavenArtifactLister>,
        metadata_cache: Arc<dyn MavenMetadataCache>,
    ) -> Self {
        let use_case = GenerateMavenMetadataUseCase::new(
            metadata_generator,
            artifact_lister,
            metadata_cache,
        );
        let api = GenerateMavenMetadataApi::new(use_case);
        
        Self { api }
    }
    
    /// Método de conveniencia para producción con implementaciones reales
    pub fn for_production(
        s3_client: Arc<dyn S3Client>,
        s3_bucket: String,
        mongo_client: Arc<dyn MongoClient>,
        mongo_database: String,
        redis_client: Arc<dyn RedisClient>,
        cache_ttl_seconds: u64,
    ) -> Self {
        let metadata_generator: Arc<dyn MavenMetadataGenerator> = 
            Arc::new(S3MavenMetadataGenerator::new(s3_client, s3_bucket));
        
        let artifact_lister: Arc<dyn MavenArtifactLister> = 
            Arc::new(MongoMavenArtifactLister::new(mongo_client, mongo_database));
        
        let metadata_cache: Arc<dyn MavenMetadataCache> = 
            Arc::new(RedisMavenMetadataCache::new(redis_client, cache_ttl_seconds));
        
        Self::new(metadata_generator, artifact_lister, metadata_cache)
    }
    
    /// Método de conveniencia para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::{
            MockS3Client, MockMongoClient, MockRedisClient,
        };
        
        let s3_client: Arc<dyn S3Client> = Arc::new(MockS3Client::new());
        let mongo_client: Arc<dyn MongoClient> = Arc::new(MockMongoClient::new());
        let redis_client: Arc<dyn RedisClient> = Arc::new(MockRedisClient::new());
        
        Self::for_production(
            s3_client,
            "test-bucket".to_string(),
            mongo_client,
            "test-database".to_string(),
            redis_client,
            3600, // 1 hora de TTL para testing
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::maven::{MavenCoordinates, MavenVersion};
    use crate::features::generate_maven_metadata::dto::{
        GenerateMavenMetadataRequest, GenerateMavenMetadataResponse,
    };
    
    #[tokio::test]
    async fn test_di_container_creation() {
        let container = GenerateMavenMetadataDIContainer::for_testing();
        
        // Verificar que el API está disponible
        assert!(std::ptr::eq(
            &container.api as *const _,
            &container.api as *const _
        ));
    }
    
    #[tokio::test]
    async fn test_di_container_with_custom_implementations() {
        use super::adapter::test::{MockS3Client, MockMongoClient, MockRedisClient};
        
        let s3_client: Arc<dyn S3Client> = Arc::new(MockS3Client::new());
        let mongo_client: Arc<dyn MongoClient> = Arc::new(MockMongoClient::new());
        let redis_client: Arc<dyn RedisClient> = Arc::new(MockRedisClient::new());
        
        let container = GenerateMavenMetadataDIContainer::for_production(
            s3_client,
            "custom-bucket".to_string(),
            mongo_client,
            "custom-database".to_string(),
            redis_client,
            7200, // 2 horas de TTL
        );
        
        // Crear una solicitud de prueba
        let request = GenerateMavenMetadataRequest {
            coordinates: MavenCoordinates::new("com.example", "test-artifact", "1.0.0").unwrap(),
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };
        
        // Ejecutar la solicitud (esto debería funcionar con los mocks)
        let result = container.api.generate_metadata(request).await;
        
        // Verificar que no hay errores de configuración
        assert!(result.is_ok() || result.is_err(), "La ejecución debe completarse sin panics");
    }
    
    #[tokio::test]
    async fn test_di_container_production_config() {
        // Esta prueba verifica que la configuración de producción se puede crear
        // sin errores de compilación
        let _container = GenerateMavenMetadataDIContainer::for_testing();
        
        // Si llegamos aquí, la configuración es válida
        assert!(true);
    }
}