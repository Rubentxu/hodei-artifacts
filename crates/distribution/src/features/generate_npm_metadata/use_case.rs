// crates/distribution/src/features/generate_npm_metadata/use_case.rs

//! Caso de uso para generar metadatos npm

use std::sync::Arc;
use tracing::{info, warn, error, instrument};
use crate::domain::npm::{NpmPackageName, NpmVersion};
use super::dto::{
    GenerateNpmMetadataRequest, GenerateNpmMetadataResponse, NpmPackageMetadataDto,
    GenerateNpmMetadataError,
};
use super::ports::{
    NpmMetadataGenerator, NpmPackageLister, NpmMetadataCache,
    MetadataGeneratorError, PackageListerError, MetadataCacheError,
};

/// Caso de uso para generar metadatos npm
pub struct GenerateNpmMetadataUseCase {
    metadata_generator: Arc<dyn NpmMetadataGenerator>,
    package_lister: Arc<dyn NpmPackageLister>,
    metadata_cache: Arc<dyn NpmMetadataCache>,
    default_ttl_seconds: u64,
}

impl GenerateNpmMetadataUseCase {
    /// Crea una nueva instancia del caso de uso
    pub fn new(
        metadata_generator: Arc<dyn NpmMetadataGenerator>,
        package_lister: Arc<dyn NpmPackageLister>,
        metadata_cache: Arc<dyn NpmMetadataCache>,
        default_ttl_seconds: u64,
    ) -> Self {
        Self {
            metadata_generator,
            package_lister,
            metadata_cache,
            default_ttl_seconds,
        }
    }

    /// Ejecuta el caso de uso para generar metadatos npm
    #[instrument(
        skip(self, request),
        fields(
            package_name = %request.package_name,
            repository_id = %request.repository_id,
            force_regenerate = request.force_regenerate
        )
    )]
    pub async fn execute(
        &self,
        request: GenerateNpmMetadataRequest,
    ) -> Result<GenerateNpmMetadataResponse, GenerateNpmMetadataError> {
        info!("Generating npm metadata for package: {}", request.package_name);

        // Validar el nombre del paquete
        self.validate_package_name(&request.package_name)?;

        // Construir el nombre completo del paquete (incluyendo scope si existe)
        let full_package_name = if let Some(scope) = &request.scope {
            format!("{}/{}", scope, request.package_name)
        } else {
            request.package_name.clone()
        };

        let package_name = NpmPackageName::new(&full_package_name)
            .map_err(|e| GenerateNpmMetadataError::InvalidPackageName {
                name: full_package_name.clone(),
            })?;

        // Verificar si se debe forzar la regeneración
        if !request.force_regenerate {
            // Intentar obtener desde caché
            match self.get_cached_metadata(&package_name, &request.repository_id).await {
                Ok(Some(cached_response)) => {
                    info!("Returning cached metadata for package: {}", full_package_name);
                    return Ok(cached_response);
                }
                Ok(None) => {
                    info!("No cached metadata found for package: {}", full_package_name);
                }
                Err(e) => {
                    warn!("Error retrieving cached metadata: {}. Proceeding with generation.", e);
                }
            }
        } else {
            info!("Force regenerate requested for package: {}", full_package_name);
        }

        // Generar nuevos metadatos
        let metadata = self.generate_metadata(&package_name, &request.repository_id).await?;

        // Obtener información adicional del paquete
        let package_info = self.get_package_info(&package_name, &request.repository_id).await?;

        // Combinar metadatos con información del paquete
        let combined_metadata = self.combine_metadata(metadata, package_info)?;

        // Guardar en caché
        if let Err(e) = self.cache_metadata(&package_name, &request.repository_id, &combined_metadata).await {
            warn!("Error caching metadata: {}. Continuing with response.", e);
        }

        let response = GenerateNpmMetadataResponse {
            metadata: combined_metadata,
            generated_at: chrono::Utc::now().to_rfc3339(),
            cache_hit: false,
        };

        info!("Successfully generated metadata for package: {}", full_package_name);
        Ok(response)
    }

    /// Valida el nombre del paquete
    fn validate_package_name(&self, package_name: &str) -> Result<(), GenerateNpmMetadataError> {
        crate::features::generate_npm_metadata::dto::validate_npm_package_name(package_name)
    }

    /// Intenta obtener metadatos desde caché
    #[instrument(skip(self))]
    async fn get_cached_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<Option<GenerateNpmMetadataResponse>, GenerateNpmMetadataError> {
        let cached = self.metadata_cache
            .get_cached_metadata(package_name, repository_id)
            .await
            .map_err(|e| GenerateNpmMetadataError::CacheError(e.to_string()))?;

        if let Some(cached_metadata) = cached {
            // Verificar si el caché aún es válido
            let cached_at = chrono::DateTime::parse_from_rfc3339(&cached_metadata.cached_at)
                .map_err(|e| GenerateNpmMetadataError::CacheError(format!("Invalid cache timestamp: {}", e)))?;
            
            let now = chrono::Utc::now();
            let age_seconds = (now - cached_at).num_seconds();
            
            if age_seconds < cached_metadata.ttl_seconds as i64 {
                return Ok(Some(GenerateNpmMetadataResponse {
                    metadata: cached_metadata.metadata,
                    generated_at: cached_metadata.cached_at,
                    cache_hit: true,
                }));
            }
        }

        Ok(None)
    }

    /// Genera nuevos metadatos
    #[instrument(skip(self))]
    async fn generate_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<NpmPackageMetadataDto, GenerateNpmMetadataError> {
        self.metadata_generator
            .generate_metadata(package_name, repository_id)
            .await
            .map_err(|e| match e {
                MetadataGeneratorError::PackageNotFound { package_name } => {
                    GenerateNpmMetadataError::PackageNotFound {
                        package_name,
                        repository_id: repository_id.to_string(),
                    }
                }
                MetadataGeneratorError::InvalidPackageName { name } => {
                    GenerateNpmMetadataError::InvalidPackageName { name }
                }
                MetadataGeneratorError::GenerationFailed { reason } => {
                    GenerateNpmMetadataError::MetadataGenerationFailed { reason }
                }
                MetadataGeneratorError::RepositoryError(msg) => {
                    GenerateNpmMetadataError::RepositoryError(msg)
                }
            })
    }

    /// Obtiene información del paquete
    #[instrument(skip(self))]
    async fn get_package_info(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<super::ports::PackageInfo, GenerateNpmMetadataError> {
        // Obtener todas las versiones del paquete
        let versions = self.package_lister
            .list_package_versions(package_name, repository_id)
            .await
            .map_err(|e| match e {
                PackageListerError::PackageNotFound { package_name } => {
                    GenerateNpmMetadataError::PackageNotFound {
                        package_name,
                        repository_id: repository_id.to_string(),
                    }
                }
                PackageListerError::RepositoryNotFound { repository_id } => {
                    GenerateNpmMetadataError::RepositoryError(format!("Repository not found: {}", repository_id))
                }
                PackageListerError::RepositoryError(msg) => {
                    GenerateNpmMetadataError::RepositoryError(msg)
                }
            })?;

        // Obtener información de la última versión
        let latest_version = versions.last()
            .ok_or_else(|| GenerateNpmMetadataError::PackageNotFound {
                package_name: package_name.to_string(),
                repository_id: repository_id.to_string(),
            })?;

        let package_info = self.package_lister
            .get_package_info(package_name, latest_version, repository_id)
            .await
            .map_err(|e| match e {
                PackageListerError::PackageNotFound { package_name } => {
                    GenerateNpmMetadataError::PackageNotFound {
                        package_name,
                        repository_id: repository_id.to_string(),
                    }
                }
                PackageListerError::RepositoryNotFound { repository_id } => {
                    GenerateNpmMetadataError::RepositoryError(format!("Repository not found: {}", repository_id))
                }
                PackageListerError::RepositoryError(msg) => {
                    GenerateNpmMetadataError::RepositoryError(msg)
                }
            })?;

        Ok(package_info)
    }

    /// Combina metadatos con información del paquete
    fn combine_metadata(
        &self,
        mut metadata: NpmPackageMetadataDto,
        package_info: super::ports::PackageInfo,
    ) -> Result<NpmPackageMetadataDto, GenerateNpmMetadataError> {
        // Actualizar campos con información adicional
        metadata.description = package_info.description.or_else(|| metadata.description);
        metadata.keywords = package_info.keywords.or_else(|| metadata.keywords);
        metadata.homepage = package_info.homepage.or_else(|| metadata.homepage);
        metadata.author = package_info.author.map(|author| match author {
            super::ports::AuthorInfo::String(s) => super::dto::AuthorDto::String(s),
            super::ports::AuthorInfo::Object { name, email, url } => super::dto::AuthorDto::Object { name, email, url },
        }).or_else(|| metadata.author);
        metadata.license = package_info.license.or_else(|| metadata.license);
        metadata.dependencies = package_info.dependencies.or_else(|| metadata.dependencies);
        metadata.dev_dependencies = package_info.dev_dependencies.or_else(|| metadata.dev_dependencies);
        metadata.optional_dependencies = package_info.optional_dependencies.or_else(|| metadata.optional_dependencies);
        metadata.peer_dependencies = package_info.peer_dependencies.or_else(|| metadata.peer_dependencies);
        metadata.scripts = package_info.scripts.or_else(|| metadata.scripts);
        metadata.main = package_info.main.or_else(|| metadata.main);
        metadata.bin = package_info.bin.map(|bin| match bin {
            super::ports::BinInfo::Single(s) => super::dto::BinDto::Single(s),
            super::ports::BinInfo::Multiple(m) => super::dto::BinDto::Multiple(m),
        }).or_else(|| metadata.bin);
        metadata.files = package_info.files.or_else(|| metadata.files);
        metadata.engines = package_info.engines.or_else(|| metadata.engines);
        metadata.os = package_info.os.or_else(|| metadata.os);
        metadata.cpu = package_info.cpu.or_else(|| metadata.cpu);
        metadata.private = package_info.private.or_else(|| metadata.private);
        metadata.publish_config = package_info.publish_config.map(|config| super::dto::PublishConfigDto {
            registry: config.registry,
            ignore: config.ignore,
            include: config.include,
            access: config.access,
            tag: config.tag,
        }).or_else(|| metadata.publish_config);
        metadata.dist_tags = package_info.dist_tags;
        metadata.time = package_info.published_at.map(|published_at| {
            let mut time_map = std::collections::HashMap::new();
            time_map.insert("created".to_string(), published_at);
            time_map.insert("modified".to_string(), chrono::Utc::now().to_rfc3339());
            time_map
        }).or_else(|| metadata.time);
        metadata.dist = package_info.dist.map(|dist| super::dto::DistDto {
            integrity: dist.integrity,
            tarball: dist.tarball,
            file_count: dist.file_count,
            unpacked_size: dist.unpacked_size,
            shasum: dist.shasum,
            size: dist.size,
        }).or_else(|| metadata.dist);

        Ok(metadata)
    }

    /// Guarda metadatos en caché
    #[instrument(skip(self))]
    async fn cache_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
        metadata: &NpmPackageMetadataDto,
    ) -> Result<(), GenerateNpmMetadataError> {
        self.metadata_cache
            .cache_metadata(package_name, repository_id, metadata, self.default_ttl_seconds)
            .await
            .map_err(|e| GenerateNpmMetadataError::CacheError(e.to_string()))?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::NpmPackageName;
    use crate::features::generate_npm_metadata::ports::test::{
        MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache,
    };

    fn create_test_use_case() -> GenerateNpmMetadataUseCase {
        let metadata_generator = Arc::new(MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: true,
        });
        let package_lister = Arc::new(MockNpmPackageLister::new());
        let metadata_cache = Arc::new(MockNpmMetadataCache::new());
        
        GenerateNpmMetadataUseCase::new(
            metadata_generator,
            package_lister,
            metadata_cache,
            3600, // 1 hour TTL
        )
    }

    #[tokio::test]
    async fn test_execute_success() {
        let use_case = create_test_use_case();
        
        let request = GenerateNpmMetadataRequest {
            scope: None,
            package_name: "test-package".to_string(),
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };

        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.metadata.name, "test-package");
        assert_eq!(response.metadata.version, "1.0.0");
        assert!(!response.cache_hit);
    }

    #[tokio::test]
    async fn test_execute_with_scope() {
        let use_case = create_test_use_case();
        
        let request = GenerateNpmMetadataRequest {
            scope: Some("@myorg".to_string()),
            package_name: "test-package".to_string(),
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };

        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.metadata.name, "@myorg/test-package");
    }

    #[tokio::test]
    async fn test_execute_invalid_package_name() {
        let use_case = create_test_use_case();
        
        let request = GenerateNpmMetadataRequest {
            scope: None,
            package_name: "".to_string(), // Invalid name
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };

        let result = use_case.execute(request).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            GenerateNpmMetadataError::InvalidPackageName { name } => {
                assert_eq!(name, "");
            }
            _ => panic!("Expected InvalidPackageName error"),
        }
    }

    #[tokio::test]
    async fn test_execute_force_regenerate() {
        let use_case = create_test_use_case();
        
        let request = GenerateNpmMetadataRequest {
            scope: None,
            package_name: "test-package".to_string(),
            repository_id: "test-repo".to_string(),
            force_regenerate: true,
        };

        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.cache_hit); // Should not hit cache when force regenerate
    }

    #[tokio::test]
    async fn test_execute_package_not_found() {
        let metadata_generator = Arc::new(MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: false, // Package doesn't exist
        });
        let package_lister = Arc::new(MockNpmPackageLister::new());
        let metadata_cache = Arc::new(MockNpmMetadataCache::new());
        
        let use_case = GenerateNpmMetadataUseCase::new(
            metadata_generator,
            package_lister,
            metadata_cache,
            3600,
        );

        let request = GenerateNpmMetadataRequest {
            scope: None,
            package_name: "non-existent".to_string(),
            repository_id: "test-repo".to_string(),
            force_regenerate: false,
        };

        let result = use_case.execute(request).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            GenerateNpmMetadataError::PackageNotFound { package_name, repository_id } => {
                assert_eq!(package_name, "non-existent");
                assert_eq!(repository_id, "test-repo");
            }
            _ => panic!("Expected PackageNotFound error"),
        }
    }
}