// crates/distribution/src/features/generate_npm_metadata/adapter.rs

//! Adaptadores de infraestructura para generación de metadatos npm

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error, instrument};
use super::ports::{NpmMetadataGenerator, NpmPackageLister, NpmMetadataCache};
use super::dto::{NpmPackageMetadataDto, GenerateNpmMetadataError};

/// Adaptador S3 para generar metadatos npm
pub struct S3NpmMetadataGenerator {
    s3_client: Arc<dyn aws_sdk_s3::Client>,
    bucket_name: String,
}

impl S3NpmMetadataGenerator {
    /// Crea una nueva instancia del generador de metadatos npm con S3
    pub fn new(s3_client: Arc<dyn aws_sdk_s3::Client>, bucket_name: String) -> Self {
        Self {
            s3_client,
            bucket_name,
        }
    }
}

#[async_trait]
impl NpmMetadataGenerator for S3NpmMetadataGenerator {
    #[instrument(
        skip(self, scope, package_name, repository_id),
        fields(
            scope = %scope.as_deref().unwrap_or("none"),
            package_name = %package_name,
            repository_id = %repository_id
        )
    )]
    async fn generate_metadata(
        &self,
        scope: Option<&str>,
        package_name: &str,
        repository_id: &str,
    ) -> Result<NpmPackageMetadataDto, GenerateNpmMetadataError> {
        info!(
            "Generating npm metadata for package: {} in repository: {}",
            package_name, repository_id
        );

        // Construir la clave S3 para el paquete
        let package_key = if let Some(scope) = scope {
            format!("{}/{}/package.json", repository_id, scope)
        } else {
            format!("{}/{}/package.json", repository_id, package_name)
        };

        // Intentar obtener el package.json del paquete
        let get_object_result = self
            .s3_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(&package_key)
            .send()
            .await;

        match get_object_result {
            Ok(response) => {
                let body = response.body.collect().await.map_err(|e| {
                    error!("Error reading S3 object body: {}", e);
                    GenerateNpmMetadataError::RepositoryError(format!(
                        "Error reading package.json from S3: {}",
                        e
                    ))
                })?;

                let package_json: serde_json::Value = serde_json::from_slice(&body.into_bytes())
                    .map_err(|e| {
                        error!("Error parsing package.json: {}", e);
                        GenerateNpmMetadataError::MetadataGenerationFailed {
                            reason: format!("Invalid package.json format: {}", e),
                        }
                    })?;

                // Convertir a NpmPackageMetadataDto
                let metadata = NpmPackageMetadataDto {
                    name: package_json["name"]
                        .as_str()
                        .unwrap_or(package_name)
                        .to_string(),
                    version: package_json["version"]
                        .as_str()
                        .unwrap_or("1.0.0")
                        .to_string(),
                    description: package_json["description"].as_str().map(String::from),
                    keywords: package_json["keywords"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    homepage: package_json["homepage"].as_str().map(String::from),
                    bugs: package_json["bugs"].as_str().map(String::from),
                    license: package_json["license"].as_str().map(String::from),
                    author: package_json["author"].as_str().map(String::from),
                    contributors: package_json["contributors"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    files: package_json["files"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    main: package_json["main"].as_str().map(String::from),
                    bin: package_json["bin"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    man: package_json["man"].as_str().map(String::from),
                    directories: package_json["directories"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    repository: package_json["repository"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    scripts: package_json["scripts"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    config: package_json["config"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    dependencies: package_json["dependencies"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    dev_dependencies: package_json["devDependencies"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    peer_dependencies: package_json["peerDependencies"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    bundled_dependencies: package_json["bundledDependencies"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    optional_dependencies: package_json["optionalDependencies"]
                        .as_object()
                        .map(|obj| {
                            obj.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                                .collect()
                        }),
                    engines: package_json["engines"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    os: package_json["os"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    cpu: package_json["cpu"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                    private: package_json["private"].as_bool().unwrap_or(false),
                    publish_config: package_json["publishConfig"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    dist_tags: package_json["dist-tags"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    versions: package_json["versions"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    time: package_json["time"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    }),
                    users: package_json["users"].as_object().map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(false)))
                            .collect()
                    }),
                    readme: package_json["readme"].as_str().map(String::from),
                    readme_filename: package_json["readmeFilename"].as_str().map(String::from),
                    _id: package_json["_id"].as_str().map(String::from),
                    _rev: package_json["_rev"].as_str().map(String::from),
                };

                info!("Successfully generated npm metadata for package: {}", package_name);
                Ok(metadata)
            }
            Err(e) => {
                error!("Error getting package.json from S3: {}", e);
                Err(GenerateNpmMetadataError::PackageNotFound {
                    package_name: package_name.to_string(),
                    repository_id: repository_id.to_string(),
                })
            }
        }
    }
}

/// Adaptador MongoDB para listar paquetes npm
pub struct MongoNpmPackageLister {
    mongo_client: Arc<dyn mongodb::Client>,
    database_name: String,
}

impl MongoNpmPackageLister {
    /// Crea una nueva instancia del listador de paquetes npm con MongoDB
    pub fn new(mongo_client: Arc<dyn mongodb::Client>, database_name: String) -> Self {
        Self {
            mongo_client,
            database_name,
        }
    }
}

#[async_trait]
impl NpmPackageLister for MongoNpmPackageLister {
    #[instrument(
        skip(self, repository_id),
        fields(repository_id = %repository_id)
    )]
    async fn list_packages(
        &self,
        repository_id: &str,
    ) -> Result<Vec<String>, GenerateNpmMetadataError> {
        info!("Listing npm packages in repository: {}", repository_id);

        let collection = self
            .mongo_client
            .database(&self.database_name)
            .collection::<mongodb::bson::Document>("npm_packages");

        let filter = mongodb::bson::doc! { "repository_id": repository_id };
        let mut cursor = collection.find(filter).await.map_err(|e| {
            error!("Error querying MongoDB: {}", e);
            GenerateNpmMetadataError::RepositoryError(format!("MongoDB query error: {}", e))
        })?;

        let mut packages = Vec::new();
        while cursor.advance().await.map_err(|e| {
            error!("Error advancing MongoDB cursor: {}", e);
            GenerateNpmMetadataError::RepositoryError(format!("MongoDB cursor error: {}", e))
        })? {
            let doc = cursor.current();
            if let Some(name) = doc.get_str("name").ok() {
                packages.push(name.to_string());
            }
        }

        info!("Found {} npm packages in repository", packages.len());
        Ok(packages)
    }
}

/// Adaptador Redis para caché de metadatos npm
pub struct RedisNpmMetadataCache {
    redis_client: Arc<dyn redis::aio::Connection>,
    ttl_seconds: u64,
}

impl RedisNpmMetadataCache {
    /// Crea una nueva instancia del caché de metadatos npm con Redis
    pub fn new(redis_client: Arc<dyn redis::aio::Connection>, ttl_seconds: u64) -> Self {
        Self {
            redis_client,
            ttl_seconds,
        }
    }

    /// Genera una clave de caché para los metadatos npm
    fn cache_key(&self, scope: Option<&str>, package_name: &str, repository_id: &str) -> String {
        if let Some(scope) = scope {
            format!("npm:metadata:{}:{}:{}", repository_id, scope, package_name)
        } else {
            format!("npm:metadata:{}:{}", repository_id, package_name)
        }
    }
}

#[async_trait]
impl NpmMetadataCache for RedisNpmMetadataCache {
    #[instrument(
        skip(self, scope, package_name, repository_id),
        fields(
            scope = %scope.as_deref().unwrap_or("none"),
            package_name = %package_name,
            repository_id = %repository_id
        )
    )]
    async fn get_metadata(
        &self,
        scope: Option<&str>,
        package_name: &str,
        repository_id: &str,
    ) -> Result<Option<NpmPackageMetadataDto>, GenerateNpmMetadataError> {
        let key = self.cache_key(scope, package_name, repository_id);
        
        let cached_data: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut *self.redis_client.clone())
            .await
            .map_err(|e| {
                error!("Error getting from Redis cache: {}", e);
                GenerateNpmMetadataError::CacheError(format!("Redis GET error: {}", e))
            })?;

        if let Some(data) = cached_data {
            let metadata: NpmPackageMetadataDto = serde_json::from_str(&data).map_err(|e| {
                error!("Error deserializing cached metadata: {}", e);
                GenerateNpmMetadataError::CacheError(format!("JSON deserialization error: {}", e))
            })?;

            info!("Cache hit for npm metadata: {}", package_name);
            Ok(Some(metadata))
        } else {
            info!("Cache miss for npm metadata: {}", package_name);
            Ok(None)
        }
    }

    #[instrument(
        skip(self, scope, package_name, repository_id, metadata),
        fields(
            scope = %scope.as_deref().unwrap_or("none"),
            package_name = %package_name,
            repository_id = %repository_id
        )
    )]
    async fn set_metadata(
        &self,
        scope: Option<&str>,
        package_name: &str,
        repository_id: &str,
        metadata: &NpmPackageMetadataDto,
    ) -> Result<(), GenerateNpmMetadataError> {
        let key = self.cache_key(scope, package_name, repository_id);
        
        let data = serde_json::to_string(metadata).map_err(|e| {
            error!("Error serializing metadata for cache: {}", e);
            GenerateNpmMetadataError::CacheError(format!("JSON serialization error: {}", e))
        })?;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(self.ttl_seconds)
            .arg(&data)
            .query_async(&mut *self.redis_client.clone())
            .await
            .map_err(|e| {
                error!("Error setting in Redis cache: {}", e);
                GenerateNpmMetadataError::CacheError(format!("Redis SETEX error: {}", e))
            })?;

        info!("Cached npm metadata for package: {}", package_name);
        Ok(())
    }

    #[instrument(
        skip(self, scope, package_name, repository_id),
        fields(
            scope = %scope.as_deref().unwrap_or("none"),
            package_name = %package_name,
            repository_id = %repository_id
        )
    )]
    async fn invalidate_metadata(
        &self,
        scope: Option<&str>,
        package_name: &str,
        repository_id: &str,
    ) -> Result<(), GenerateNpmMetadataError> {
        let key = self.cache_key(scope, package_name, repository_id);
        
        redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut *self.redis_client.clone())
            .await
            .map_err(|e| {
                error!("Error deleting from Redis cache: {}", e);
                GenerateNpmMetadataError::CacheError(format!("Redis DEL error: {}", e))
            })?;

        info!("Invalidated cache for npm metadata: {}", package_name);
        Ok(())
    }
}

/// Adaptador en memoria para pruebas
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;

    /// Generador de metadatos npm mock para pruebas
    pub struct MockNpmMetadataGenerator {
        pub should_fail: bool,
        pub package_exists: bool,
    }

    #[async_trait]
    impl NpmMetadataGenerator for MockNpmMetadataGenerator {
        async fn generate_metadata(
            &self,
            scope: Option<&str>,
            package_name: &str,
            repository_id: &str,
        ) -> Result<NpmPackageMetadataDto, GenerateNpmMetadataError> {
            if self.should_fail {
                return Err(GenerateNpmMetadataError::MetadataGenerationFailed {
                    reason: "Mock failure".to_string(),
                });
            }

            if !self.package_exists {
                return Err(GenerateNpmMetadataError::PackageNotFound {
                    package_name: package_name.to_string(),
                    repository_id: repository_id.to_string(),
                });
            }

            let full_name = if let Some(scope) = scope {
                format!("{}/{}", scope, package_name)
            } else {
                package_name.to_string()
            };

            Ok(NpmPackageMetadataDto {
                name: full_name,
                version: "1.0.0".to_string(),
                description: Some("Test package".to_string()),
                keywords: vec!["test".to_string(), "npm".to_string()],
                homepage: Some("https://example.com".to_string()),
                bugs: Some("https://github.com/example/issues".to_string()),
                license: Some("MIT".to_string()),
                author: Some("Test Author".to_string()),
                contributors: vec!["Contributor 1".to_string()],
                files: vec!["index.js".to_string(), "README.md".to_string()],
                main: Some("index.js".to_string()),
                bin: None,
                man: None,
                directories: None,
                repository: None,
                scripts: None,
                config: None,
                dependencies: None,
                dev_dependencies: None,
                peer_dependencies: None,
                bundled_dependencies: vec![],
                optional_dependencies: None,
                engines: None,
                os: vec![],
                cpu: vec![],
                private: false,
                publish_config: None,
                dist_tags: None,
                versions: None,
                time: None,
                users: None,
                readme: Some("# Test Package\n\nThis is a test package.".to_string()),
                readme_filename: Some("README.md".to_string()),
                _id: Some("test-package".to_string()),
                _rev: Some("1-abc123".to_string()),
            })
        }
    }

    /// Listador de paquetes npm mock para pruebas
    pub struct MockNpmPackageLister {
        packages: Mutex<Vec<String>>,
    }

    impl MockNpmPackageLister {
        pub fn new() -> Self {
            Self {
                packages: Mutex::new(vec![
                    "test-package".to_string(),
                    "another-package".to_string(),
                    "@scope/scoped-package".to_string(),
                ]),
            }
        }
    }

    #[async_trait]
    impl NpmPackageLister for MockNpmPackageLister {
        async fn list_packages(
            &self,
            _repository_id: &str,
        ) -> Result<Vec<String>, GenerateNpmMetadataError> {
            let packages = self.packages.lock().unwrap().clone();
            Ok(packages)
        }
    }

    /// Caché de metadatos npm mock para pruebas
    pub struct MockNpmMetadataCache {
        cache: Mutex<HashMap<String, NpmPackageMetadataDto>>,
    }

    impl MockNpmMetadataCache {
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl NpmMetadataCache for MockNpmMetadataCache {
        async fn get_metadata(
            &self,
            scope: Option<&str>,
            package_name: &str,
            repository_id: &str,
        ) -> Result<Option<NpmPackageMetadataDto>, GenerateNpmMetadataError> {
            let key = if let Some(scope) = scope {
                format!("{}:{}:{}", repository_id, scope, package_name)
            } else {
                format!("{}:{}", repository_id, package_name)
            };

            Ok(self.cache.lock().unwrap().get(&key).cloned())
        }

        async fn set_metadata(
            &self,
            scope: Option<&str>,
            package_name: &str,
            repository_id: &str,
            metadata: &NpmPackageMetadataDto,
        ) -> Result<(), GenerateNpmMetadataError> {
            let key = if let Some(scope) = scope {
                format!("{}:{}:{}", repository_id, scope, package_name)
            } else {
                format!("{}:{}", repository_id, package_name)
            };

            self.cache.lock().unwrap().insert(key, metadata.clone());
            Ok(())
        }

        async fn invalidate_metadata(
            &self,
            scope: Option<&str>,
            package_name: &str,
            repository_id: &str,
        ) -> Result<(), GenerateNpmMetadataError> {
            let key = if let Some(scope) = scope {
                format!("{}:{}:{}", repository_id, scope, package_name)
            } else {
                format!("{}:{}", repository_id, package_name)
            };

            self.cache.lock().unwrap().remove(&key);
            Ok(())
        }
    }
}