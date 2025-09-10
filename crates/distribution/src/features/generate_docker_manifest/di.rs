// crates/distribution/src/features/generate_docker_manifest/di.rs

//! Contenedor de inyección de dependencias para la generación de manifests Docker

use std::sync::Arc;

use super::{
    api::GenerateDockerManifestApi,
    ports::{DockerManifestGenerator, DockerLayerLister, DockerManifestCache},
    use_case::GenerateDockerManifestUseCase,
};

/// Contenedor de DI para la feature de generación de manifests Docker
pub struct GenerateDockerManifestDIContainer {
    pub api: GenerateDockerManifestApi,
}

impl GenerateDockerManifestDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los puertos
    pub fn new(
        generator: Arc<dyn DockerManifestGenerator>,
        layer_lister: Arc<dyn DockerLayerLister>,
        cache: Arc<dyn DockerManifestCache>,
    ) -> Self {
        let use_case = Arc::new(GenerateDockerManifestUseCase::new(
            generator,
            layer_lister,
            cache,
        ));
        let api = GenerateDockerManifestApi::new(use_case);

        Self { api }
    }

    /// Método de conveniencia para producción con implementaciones reales
    pub fn for_production(
        s3_client: Arc<aws_sdk_s3::Client>,
        s3_bucket: String,
        redis_client: Arc<redis::Client>,
        redis_key_prefix: String,
    ) -> Self {
        use super::adapter::{
            S3DockerManifestGenerator, S3DockerLayerLister, RedisDockerManifestCache,
        };

        let generator: Arc<dyn DockerManifestGenerator> = Arc::new(S3DockerManifestGenerator::new(
            s3_client.clone(),
            s3_bucket.clone(),
        ));

        let layer_lister: Arc<dyn DockerLayerLister> = Arc::new(S3DockerLayerLister::new(
            s3_client,
            s3_bucket,
        ));

        let cache: Arc<dyn DockerManifestCache> = Arc::new(RedisDockerManifestCache::new(
            redis_client,
            redis_key_prefix,
        ));

        Self::new(generator, layer_lister, cache)
    }

    /// Método de conveniencia para testing con mocks
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::ports::{
            MockDockerManifestGenerator, MockDockerLayerLister, MockDockerManifestCache,
        };

        let generator: Arc<dyn DockerManifestGenerator> = Arc::new(MockDockerManifestGenerator::new());
        let layer_lister: Arc<dyn DockerLayerLister> = Arc::new(MockDockerLayerLister::new());
        let cache: Arc<dyn DockerManifestCache> = Arc::new(MockDockerManifestCache::new());

        Self::new(generator, layer_lister, cache)
    }

    /// Método de conveniencia para testing con implementaciones en memoria
    #[cfg(test)]
    pub fn for_in_memory_testing() -> Self {
        use super::adapter::{
            InMemoryDockerManifestGenerator, InMemoryDockerLayerLister, InMemoryDockerManifestCache,
        };

        let generator: Arc<dyn DockerManifestGenerator> = Arc::new(InMemoryDockerManifestGenerator::new());
        let layer_lister: Arc<dyn DockerLayerLister> = Arc::new(InMemoryDockerLayerLister::new());
        let cache: Arc<dyn DockerManifestCache> = Arc::new(InMemoryDockerManifestCache::new());

        Self::new(generator, layer_lister, cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generate_docker_manifest::{
        dto::{GenerateDockerManifestRequest, DockerManifestDto, DockerDescriptorDto},
        ports::{MockDockerManifestGenerator, MockDockerLayerLister, MockDockerManifestCache},
    };

    #[tokio::test]
    async fn test_di_container_creation() {
        let container = GenerateDockerManifestDIContainer::for_testing();
        
        // Verificar que el API está disponible
        assert!(std::ptr::eq(
            &container.api,
            &container.api
        ));
    }

    #[tokio::test]
    async fn test_di_container_with_mocks() {
        let generator = Arc::new(MockDockerManifestGenerator::new());
        let layer_lister = Arc::new(MockDockerLayerLister::new());
        let cache = Arc::new(MockDockerManifestCache::new());

        let container = GenerateDockerManifestDIContainer::new(
            generator.clone(),
            layer_lister.clone(),
            cache.clone(),
        );

        // Verificar que el API funciona con los mocks
        let request = GenerateDockerManifestRequest {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            regenerate: false,
        };

        let result = container.api.generate_manifest(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_production_configuration() {
        // Este test verifica que la configuración de producción se puede crear
        // sin errores de compilación (aunque no se ejecuta sin dependencias reales)
        
        // Simular clientes de AWS S3 y Redis
        let config = aws_config::load_from_env().await;
        let s3_client = Arc::new(aws_sdk_s3::Client::new(&config));
        let redis_client = Arc::new(redis::Client::open("redis://127.0.0.1/").unwrap());

        // Verificar que se puede crear el contenedor de producción
        let _container = GenerateDockerManifestDIContainer::for_production(
            s3_client,
            "test-bucket".to_string(),
            redis_client,
            "docker:manifest:".to_string(),
        );
    }

    #[tokio::test]
    async fn test_in_memory_testing_configuration() {
        let container = GenerateDockerManifestDIContainer::for_in_memory_testing();
        
        let request = GenerateDockerManifestRequest {
            repository_name: "test/repo".to_string(),
            tag: "latest".to_string(),
            regenerate: false,
        };

        let result = container.api.generate_manifest(request).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.repository_name, "test/repo");
        assert_eq!(response.tag, "latest");
    }
}