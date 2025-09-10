// crates/distribution/src/features/generate_npm_metadata/ports.rs

//! Puertos (interfaces) para la generación de metadatos npm

use async_trait::async_trait;
use std::collections::HashMap;
use crate::domain::npm::{NpmPackageName, NpmVersion};
use super::dto::{NpmPackageMetadataDto, GenerateNpmMetadataError};

/// Generador de metadatos npm
#[async_trait]
pub trait NpmMetadataGenerator: Send + Sync {
    /// Genera metadatos para un paquete npm
    async fn generate_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<NpmPackageMetadataDto, MetadataGeneratorError>;
}

/// Listador de paquetes npm
#[async_trait]
pub trait NpmPackageLister: Send + Sync {
    /// Lista todas las versiones de un paquete npm
    async fn list_package_versions(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<Vec<NpmVersion>, PackageListerError>;
    
    /// Obtiene información de un paquete específico
    async fn get_package_info(
        &self,
        package_name: &NpmPackageName,
        version: &NpmVersion,
        repository_id: &str,
    ) -> Result<PackageInfo, PackageListerError>;
    
    /// Lista todos los paquetes en un repositorio
    async fn list_packages(
        &self,
        repository_id: &str,
    ) -> Result<Vec<NpmPackageName>, PackageListerError>;
}

/// Caché de metadatos npm
#[async_trait]
pub trait NpmMetadataCache: Send + Sync {
    /// Obtiene metadatos desde caché
    async fn get_cached_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<Option<CachedMetadata>, MetadataCacheError>;
    
    /// Guarda metadatos en caché
    async fn cache_metadata(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
        metadata: &NpmPackageMetadataDto,
        ttl_seconds: u64,
    ) -> Result<(), MetadataCacheError>;
    
    /// Invalida caché para un paquete
    async fn invalidate_cache(
        &self,
        package_name: &NpmPackageName,
        repository_id: &str,
    ) -> Result<(), MetadataCacheError>;
    
    /// Invalida todo el caché de un repositorio
    async fn invalidate_repository_cache(
        &self,
        repository_id: &str,
    ) -> Result<(), MetadataCacheError>;
}

/// Información de un paquete npm
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// Nombre del paquete
    pub name: String,
    /// Versión del paquete
    pub version: String,
    /// Descripción
    pub description: Option<String>,
    /// Palabras clave
    pub keywords: Option<Vec<String>>,
    /// Página principal
    pub homepage: Option<String>,
    /// URL del repositorio
    pub repository: Option<RepositoryInfo>,
    /// Autor
    pub author: Option<AuthorInfo>,
    /// Licencia
    pub license: Option<String>,
    /// Dependencias
    pub dependencies: Option<HashMap<String, String>>,
    /// Dependencias de desarrollo
    pub dev_dependencies: Option<HashMap<String, String>>,
    /// Dependencias opcionales
    pub optional_dependencies: Option<HashMap<String, String>>,
    /// Dependencias de peer
    pub peer_dependencies: Option<HashMap<String, String>>,
    /// Scripts
    pub scripts: Option<HashMap<String, String>>,
    /// Archivo principal
    pub main: Option<String>,
    /// Punto de entrada binario
    pub bin: Option<BinInfo>,
    /// Archivos incluidos
    pub files: Option<Vec<String>>,
    /// Motores soportados
    pub engines: Option<HashMap<String, String>>,
    /// Sistema operativo soportado
    pub os: Option<Vec<String>>,
    /// CPU soportada
    pub cpu: Option<Vec<String>>,
    /// ¿Es privado?
    pub private: Option<bool>,
    /// Configuración de publicación
    pub publish_config: Option<PublishConfigInfo>,
    /// Dist-tags
    pub dist_tags: HashMap<String, String>,
    /// Tiempo de publicación
    pub published_at: Option<String>,
    /// Información de distribución
    pub dist: Option<DistInfo>,
}

/// Información del repositorio
#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    /// Tipo de repositorio
    pub repo_type: String,
    /// URL del repositorio
    pub url: String,
    /// Directorio dentro del repositorio
    pub directory: Option<String>,
}

/// Información del autor
#[derive(Debug, Clone)]
pub enum AuthorInfo {
    /// Autor como string
    String(String),
    /// Autor como objeto
    Object {
        name: String,
        email: Option<String>,
        url: Option<String>,
    },
}

/// Información binaria
#[derive(Debug, Clone)]
pub enum BinInfo {
    /// Binario único
    Single(String),
    /// Múltiples binarios
    Multiple(HashMap<String, String>),
}

/// Información de configuración de publicación
#[derive(Debug, Clone)]
pub struct PublishConfigInfo {
    /// Registro de publicación
    pub registry: Option<String>,
    /// ¿Ignorar archivos en .npmignore?
    pub ignore: Option<Vec<String>>,
    /// ¿Incluir solo archivos específicos?
    pub include: Option<Vec<String>>,
    /// ¿Es accesible públicamente?
    pub access: Option<String>,
    /// Tag de publicación
    pub tag: Option<String>,
}

/// Información de distribución
#[derive(Debug, Clone)]
pub struct DistInfo {
    /// Integridad del archivo
    pub integrity: Option<String>,
    /// URL de descarga del tarball
    pub tarball: String,
    /// Número de archivos en el tarball
    pub file_count: Option<u64>,
    /// Tamaño descomprimido
    pub unpacked_size: Option<u64>,
    /// Suma de verificación
    pub shasum: Option<String>,
    /// Tamaño del tarball
    pub size: Option<u64>,
}

/// Metadatos en caché
#[derive(Debug, Clone)]
pub struct CachedMetadata {
    /// Metadatos del paquete
    pub metadata: NpmPackageMetadataDto,
    /// Timestamp de cuando se guardó en caché
    pub cached_at: String,
    /// TTL en segundos
    pub ttl_seconds: u64,
}

/// Errores del generador de metadatos
#[derive(Debug, thiserror::Error)]
pub enum MetadataGeneratorError {
    #[error("Package not found: {package_name}")]
    PackageNotFound { package_name: String },
    
    #[error("Invalid package name: {name}")]
    InvalidPackageName { name: String },
    
    #[error("Metadata generation failed: {reason}")]
    GenerationFailed { reason: String },
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

/// Errores del listador de paquetes
#[derive(Debug, thiserror::Error)]
pub enum PackageListerError {
    #[error("Package not found: {package_name}")]
    PackageNotFound { package_name: String },
    
    #[error("Repository not found: {repository_id}")]
    RepositoryNotFound { repository_id: String },
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

/// Errores del caché de metadatos
#[derive(Debug, thiserror::Error)]
pub enum MetadataCacheError {
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
}

/// Implementaciones mock para testing
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;

    /// Mock del generador de metadatos
    pub struct MockNpmMetadataGenerator {
        pub should_fail: bool,
        pub package_exists: bool,
    }

    #[async_trait]
    impl NpmMetadataGenerator for MockNpmMetadataGenerator {
        async fn generate_metadata(
            &self,
            package_name: &NpmPackageName,
            repository_id: &str,
        ) -> Result<NpmPackageMetadataDto, MetadataGeneratorError> {
            if self.should_fail {
                return Err(MetadataGeneratorError::GenerationFailed {
                    reason: "Mock failure".to_string(),
                });
            }

            if !self.package_exists {
                return Err(MetadataGeneratorError::PackageNotFound {
                    package_name: package_name.to_string(),
                });
            }

            Ok(NpmPackageMetadataDto {
                name: package_name.to_string(),
                description: Some("Mock package description".to_string()),
                version: "1.0.0".to_string(),
                keywords: Some(vec!["mock".to_string(), "test".to_string()]),
                homepage: Some("https://example.com".to_string()),
                repository: Some(super::RepositoryInfo {
                    repo_type: "git".to_string(),
                    url: "https://github.com/user/repo.git".to_string(),
                    directory: None,
                }),
                author: Some(super::AuthorInfo::String("Mock Author".to_string())),
                license: Some("MIT".to_string()),
                dependencies: None,
                dev_dependencies: None,
                optional_dependencies: None,
                peer_dependencies: None,
                scripts: None,
                main: Some("index.js".to_string()),
                bin: None,
                files: None,
                engines: None,
                os: None,
                cpu: None,
                private: Some(false),
                publish_config: None,
                dist_tags: HashMap::from([
                    ("latest".to_string(), "1.0.0".to_string()),
                ]),
                versions: vec!["1.0.0".to_string()],
                time: None,
                users: None,
                dist: Some(super::DistDto {
                    integrity: Some("sha512-abc123".to_string()),
                    tarball: format!("https://registry.npmjs.org/{}/-/{}-1.0.0.tgz", package_name, package_name),
                    file_count: Some(10),
                    unpacked_size: Some(1024),
                    shasum: Some("def456".to_string()),
                    size: Some(512),
                }),
            })
        }
    }

    /// Mock del listador de paquetes
    pub struct MockNpmPackageLister {
        pub packages: Mutex<HashMap<String, Vec<NpmVersion>>>,
        pub package_info: Mutex<HashMap<String, PackageInfo>>,
    }

    impl MockNpmPackageLister {
        pub fn new() -> Self {
            let mut packages = HashMap::new();
            let mut package_info = HashMap::new();

            // Agregar algunos paquetes de prueba
            let test_versions = vec![
                NpmVersion::new("1.0.0").unwrap(),
                NpmVersion::new("1.1.0").unwrap(),
                NpmVersion::new("2.0.0").unwrap(),
            ];

            packages.insert("test-package".to_string(), test_versions.clone());

            let info = PackageInfo {
                name: "test-package".to_string(),
                version: "1.0.0".to_string(),
                description: Some("Test package".to_string()),
                keywords: Some(vec!["test".to_string()]),
                homepage: Some("https://example.com".to_string()),
                repository: Some(super::RepositoryInfo {
                    repo_type: "git".to_string(),
                    url: "https://github.com/user/repo.git".to_string(),
                    directory: None,
                }),
                author: Some(super::AuthorInfo::String("Test Author".to_string())),
                license: Some("MIT".to_string()),
                dependencies: None,
                dev_dependencies: None,
                optional_dependencies: None,
                peer_dependencies: None,
                scripts: None,
                main: Some("index.js".to_string()),
                bin: None,
                files: None,
                engines: None,
                os: None,
                cpu: None,
                private: Some(false),
                publish_config: None,
                dist_tags: HashMap::from([
                    ("latest".to_string(), "1.0.0".to_string()),
                ]),
                published_at: Some("2024-01-01T12:00:00Z".to_string()),
                dist: Some(super::DistInfo {
                    integrity: Some("sha512-abc123".to_string()),
                    tarball: "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
                    file_count: Some(10),
                    unpacked_size: Some(1024),
                    shasum: Some("def456".to_string()),
                    size: Some(512),
                }),
            };

            package_info.insert("test-package-1.0.0".to_string(), info);

            Self {
                packages: Mutex::new(packages),
                package_info: Mutex::new(package_info),
            }
        }
    }

    #[async_trait]
    impl NpmPackageLister for MockNpmPackageLister {
        async fn list_package_versions(
            &self,
            package_name: &NpmPackageName,
            repository_id: &str,
        ) -> Result<Vec<NpmVersion>, PackageListerError> {
            let key = format!("{}-{}", package_name, repository_id);
            let packages = self.packages.lock().unwrap();
            
            packages.get(&key)
                .cloned()
                .ok_or_else(|| PackageListerError::PackageNotFound {
                    package_name: package_name.to_string(),
                })
        }

        async fn get_package_info(
            &self,
            package_name: &NpmPackageName,
            version: &NpmVersion,
            repository_id: &str,
        ) -> Result<PackageInfo, PackageListerError> {
            let key = format!("{}-{}-{}", package_name, version, repository_id);
            let package_info = self.package_info.lock().unwrap();
            
            package_info.get(&key)
                .cloned()
                .ok_or_else(|| PackageListerError::PackageNotFound {
                    package_name: format!("{}@{}", package_name, version),
                })
        }

        async fn list_packages(
            &self,
            repository_id: &str,
        ) -> Result<Vec<NpmPackageName>, PackageListerError> {
            if repository_id == "non-existent-repo" {
                return Err(PackageListerError::RepositoryNotFound {
                    repository_id: repository_id.to_string(),
                });
            }

            let packages = self.packages.lock().unwrap();
            let package_names: Vec<NpmPackageName> = packages.keys()
                .filter_map(|key| {
                    if key.ends_with(&format!("-{}", repository_id)) {
                        NpmPackageName::new(key.replace(&format!("-{}", repository_id), "")).ok()
                    } else {
                        None
                    }
                })
                .collect();

            Ok(package_names)
        }
    }

    /// Mock del caché de metadatos
    pub struct MockNpmMetadataCache {
        pub cache: Mutex<HashMap<String, CachedMetadata>>,
        pub should_fail: bool,
    }

    impl MockNpmMetadataCache {
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
                should_fail: false,
            }
        }
    }

    #[async_trait]
    impl NpmMetadataCache for MockNpmMetadataCache {
        async fn get_cached_metadata(
            &self,
            package_name: &NpmPackageName,
            repository_id: &str,
        ) -> Result<Option<CachedMetadata>, MetadataCacheError> {
            if self.should_fail {
                return Err(MetadataCacheError::CacheError("Mock failure".to_string()));
            }

            let key = format!("{}-{}", package_name, repository_id);
            let cache = self.cache.lock().unwrap();
            
            Ok(cache.get(&key).cloned())
        }

        async fn cache_metadata(
            &self,
            package_name: &NpmPackageName,
            repository_id: &str,
            metadata: &NpmPackageMetadataDto,
            ttl_seconds: u64,
        ) -> Result<(), MetadataCacheError> {
            if self.should_fail {
                return Err(MetadataCacheError::CacheError("Mock failure".to_string()));
            }

            let key = format!("{}-{}", package_name, repository_id);
            let cached_metadata = CachedMetadata {
                metadata: metadata.clone(),
                cached_at: chrono::Utc::now().to_rfc3339(),
                ttl_seconds,
            };

            let mut cache = self.cache.lock().unwrap();
            cache.insert(key, cached_metadata);

            Ok(())
        }

        async fn invalidate_cache(
            &self,
            package_name: &NpmPackageName,
            repository_id: &str,
        ) -> Result<(), MetadataCacheError> {
            if self.should_fail {
                return Err(MetadataCacheError::CacheError("Mock failure".to_string()));
            }

            let key = format!("{}-{}", package_name, repository_id);
            let mut cache = self.cache.lock().unwrap();
            cache.remove(&key);

            Ok(())
        }

        async fn invalidate_repository_cache(
            &self,
            repository_id: &str,
        ) -> Result<(), MetadataCacheError> {
            if self.should_fail {
                return Err(MetadataCacheError::CacheError("Mock failure".to_string()));
            }

            let mut cache = self.cache.lock().unwrap();
            cache.retain(|key, _| !key.ends_with(&format!("-{}", repository_id)));

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::NpmPackageName;

    #[test]
    fn test_package_info_creation() {
        let info = PackageInfo {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Test package".to_string()),
            keywords: Some(vec!["test".to_string()]),
            homepage: Some("https://example.com".to_string()),
            repository: Some(RepositoryInfo {
                repo_type: "git".to_string(),
                url: "https://github.com/user/repo.git".to_string(),
                directory: None,
            }),
            author: Some(AuthorInfo::String("Test Author".to_string())),
            license: Some("MIT".to_string()),
            dependencies: None,
            dev_dependencies: None,
            optional_dependencies: None,
            peer_dependencies: None,
            scripts: None,
            main: Some("index.js".to_string()),
            bin: None,
            files: None,
            engines: None,
            os: None,
            cpu: None,
            private: Some(false),
            publish_config: None,
            dist_tags: HashMap::from([
                ("latest".to_string(), "1.0.0".to_string()),
            ]),
            published_at: Some("2024-01-01T12:00:00Z".to_string()),
            dist: Some(DistInfo {
                integrity: Some("sha512-abc123".to_string()),
                tarball: "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
                file_count: Some(10),
                unpacked_size: Some(1024),
                shasum: Some("def456".to_string()),
                size: Some(512),
            }),
        };

        assert_eq!(info.name, "test-package");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.dist_tags.len(), 1);
        assert!(info.dist.is_some());
    }

    #[test]
    fn test_cached_metadata_creation() {
        let metadata = NpmPackageMetadataDto {
            name: "test-package".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            keywords: None,
            homepage: None,
            repository: None,
            author: None,
            license: None,
            dependencies: None,
            dev_dependencies: None,
            optional_dependencies: None,
            peer_dependencies: None,
            scripts: None,
            main: None,
            bin: None,
            files: None,
            engines: None,
            os: None,
            cpu: None,
            private: None,
            publish_config: None,
            dist_tags: HashMap::new(),
            versions: vec!["1.0.0".to_string()],
            time: None,
            users: None,
            dist: None,
        };

        let cached = CachedMetadata {
            metadata,
            cached_at: "2024-01-01T12:00:00Z".to_string(),
            ttl_seconds: 3600,
        };

        assert_eq!(cached.metadata.name, "test-package");
        assert_eq!(cached.ttl_seconds, 3600);
        assert_eq!(cached.cached_at, "2024-01-01T12:00:00Z");
    }

    #[tokio::test]
    async fn test_mock_metadata_generator_success() {
        let generator = test::MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: true,
        };

        let package_name = NpmPackageName::new("test-package").unwrap();
        let result = generator.generate_metadata(&package_name, "test-repo").await;

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test-package");
        assert_eq!(metadata.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_mock_metadata_generator_package_not_found() {
        let generator = test::MockNpmMetadataGenerator {
            should_fail: false,
            package_exists: false,
        };

        let package_name = NpmPackageName::new("non-existent").unwrap();
        let result = generator.generate_metadata(&package_name, "test-repo").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            MetadataGeneratorError::PackageNotFound { package_name } => {
                assert_eq!(package_name, "non-existent");
            }
            _ => panic!("Expected PackageNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_mock_package_lister_list_versions() {
        let lister = test::MockNpmPackageLister::new();
        let package_name = NpmPackageName::new("test-package").unwrap();
        
        let result = lister.list_package_versions(&package_name, "test-repo").await;
        
        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].to_string(), "1.0.0");
    }

    #[tokio::test]
    async fn test_mock_metadata_cache() {
        let cache = test::MockNpmMetadataCache::new();
        let package_name = NpmPackageName::new("test-package").unwrap();
        let metadata = NpmPackageMetadataDto {
            name: "test-package".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            keywords: None,
            homepage: None,
            repository: None,
            author: None,
            license: None,
            dependencies: None,
            dev_dependencies: None,
            optional_dependencies: None,
            peer_dependencies: None,
            scripts: None,
            main: None,
            bin: None,
            files: None,
            engines: None,
            os: None,
            cpu: None,
            private: None,
            publish_config: None,
            dist_tags: HashMap::new(),
            versions: vec!["1.0.0".to_string()],
            time: None,
            users: None,
            dist: None,
        };

        // Guardar en caché
        let result = cache.cache_metadata(&package_name, "test-repo", &metadata, 3600).await;
        assert!(result.is_ok());

        // Recuperar desde caché
        let cached = cache.get_cached_metadata(&package_name, "test-repo").await;
        assert!(cached.is_ok());
        assert!(cached.unwrap().is_some());
    }
}