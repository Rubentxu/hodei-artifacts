# Guía de Implementación VSA - Distribution Crate

## 🎯 Objetivo

Migrar la implementación actual de manejadores de formato a una arquitectura pura de Vertical Slice Architecture (VSA) con Clean Architecture, siguiendo los principios de segregación de interfaces y dominio puro.

## 📋 Pasos de Implementación

### Paso 1: Preparar la Estructura Base

```bash
# Crear estructura de directorios
mkdir -p crates/distribution/src/features/{handle_maven_request,handle_npm_request,handle_docker_request,generate_maven_metadata,generate_npm_metadata,generate_docker_manifest}/{src,tests}
mkdir -p crates/distribution/src/domain/{maven,npm,docker}/{src,tests}
mkdir -p crates/distribution/src/application/{src,tests}
mkdir -p crates/distribution/src/infrastructure/{src,tests}
```

### Paso 2: Migrar Dominio Maven a VSA

#### 2.1 Crear estructura del dominio Maven
```rust
// crates/distribution/src/domain/maven/src/coordinates.rs
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct MavenCoordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

#[derive(Debug, Error)]
pub enum MavenValidationError {
    #[error("Invalid group ID: {0}")]
    InvalidGroupId(String),
    #[error("Invalid artifact ID: {0}")]
    InvalidArtifactId(String),
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
}

impl MavenCoordinates {
    pub fn new(group_id: &str, artifact_id: &str, version: &str) -> Result<Self, MavenValidationError> {
        Self::validate_group_id(group_id)?;
        Self::validate_artifact_id(artifact_id)?;
        Self::validate_version(version)?;
        
        Ok(Self {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: "jar".to_string(),
        })
    }
    
    fn validate_group_id(group_id: &str) -> Result<(), MavenValidationError> {
        if group_id.is_empty() || !group_id.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-') {
            return Err(MavenValidationError::InvalidGroupId(group_id.to_string()));
        }
        Ok(())
    }
    
    fn validate_artifact_id(artifact_id: &str) -> Result<(), MavenValidationError> {
        if artifact_id.is_empty() || !artifact_id.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(MavenValidationError::InvalidArtifactId(artifact_id.to_string()));
        }
        Ok(())
    }
    
    fn validate_version(version: &str) -> Result<(), MavenValidationError> {
        if version.is_empty() {
            return Err(MavenValidationError::InvalidVersion(version.to_string()));
        }
        Ok(())
    }
    
    pub fn to_path(&self) -> String {
        format!("{}/{}/{}/{}-{}.{}", 
            self.group_id.replace('.', "/"),
            self.artifact_id,
            self.version,
            self.artifact_id,
            self.version,
            self.extension
        )
    }
}
```

#### 2.2 Crear feature `handle_maven_request`
```rust
// crates/distribution/src/features/handle_maven_request/src/ports.rs
use async_trait::async_trait;
use std::sync::Arc;
use crate::domain::maven::coordinates::MavenCoordinates;

/// Puerto específico para este feature - NO COMPARTIDO
#[async_trait]
pub trait MavenArtifactReader {
    async fn read_artifact(&self, coordinates: &MavenCoordinates) -> Result<Vec<u8>, MavenReadError>;
}

#[async_trait]
pub trait MavenArtifactWriter {
    async fn write_artifact(&self, coordinates: &MavenCoordinates, content: &[u8]) -> Result<(), MavenWriteError>;
}

/// Errores específicos de este feature
#[derive(Debug, thiserror::Error)]
pub enum MavenReadError {
    #[error("Artifact not found: {0}")]
    NotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum MavenWriteError {
    #[error("Write failed: {0}")]
    WriteFailed(String),
    #[error("Storage error: {0}")]
    StorageError(String),
}

/// Adaptadores para testing (solo en tests)
#[cfg(test)]
pub mod test {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    
    pub struct MockMavenArtifactReader {
        pub artifacts: Mutex<HashMap<String, Vec<u8>>>,
    }
    
    impl MockMavenArtifactReader {
        pub fn new() -> Self {
            Self {
                artifacts: Mutex::new(HashMap::new()),
            }
        }
        
        pub fn add_artifact(&self, coordinates: &MavenCoordinates, content: Vec<u8>) {
            self.artifacts.lock().unwrap().insert(coordinates.to_path(), content);
        }
    }
    
    #[async_trait]
    impl MavenArtifactReader for MockMavenArtifactReader {
        async fn read_artifact(&self, coordinates: &MavenCoordinates) -> Result<Vec<u8>, MavenReadError> {
            let path = coordinates.to_path();
            self.artifacts.lock().unwrap()
                .get(&path)
                .cloned()
                .ok_or_else(|| MavenReadError::NotFound(path))
        }
    }
}
```

#### 2.3 Crear DTOs específicos del feature
```rust
// crates/distribution/src/features/handle_maven_request/src/dto.rs
use serde::{Serialize, Deserialize};
use crate::domain::maven::coordinates::MavenCoordinates;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenGetArtifactRequest {
    pub coordinates: MavenCoordinates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenGetArtifactResponse {
    pub content: Vec<u8>,
    pub content_type: String,
    pub content_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenPutArtifactRequest {
    pub coordinates: MavenCoordinates,
    pub content: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenPutArtifactResponse {
    pub success: bool,
    pub message: String,
}
```

#### 2.4 Crear Use Case con lógica específica
```rust
// crates/distribution/src/features/handle_maven_request/src/use_case.rs
use std::sync::Arc;
use async_trait::async_trait;
use crate::domain::maven::coordinates::{MavenCoordinates, MavenValidationError};
use super::dto::{MavenGetArtifactRequest, MavenGetArtifactResponse, MavenPutArtifactRequest, MavenPutArtifactResponse};
use super::ports::{MavenArtifactReader, MavenArtifactWriter, MavenReadError, MavenWriteError};

pub struct HandleMavenGetArtifactUseCase {
    artifact_reader: Arc<dyn MavenArtifactReader>,
}

impl HandleMavenGetArtifactUseCase {
    pub fn new(artifact_reader: Arc<dyn MavenArtifactReader>) -> Self {
        Self { artifact_reader }
    }
    
    pub async fn execute(&self, request: MavenGetArtifactRequest) -> Result<MavenGetArtifactResponse, MavenGetError> {
        // Validar coordenadas (lógica de negocio pura)
        request.coordinates.validate()
            .map_err(|e| MavenGetError::InvalidCoordinates(e))?;
        
        // Leer artefacto
        let content = self.artifact_reader.read_artifact(&request.coordinates).await
            .map_err(|e| MavenGetError::ReadFailed(e))?;
        
        // Determinar content type basado en extensión
        let content_type = self.determine_content_type(&request.coordinates);
        
        Ok(MavenGetArtifactResponse {
            content,
            content_type,
            content_length: content.len(),
        })
    }
    
    fn determine_content_type(&self, coordinates: &MavenCoordinates) -> String {
        match coordinates.extension.as_str() {
            "jar" => "application/java-archive".to_string(),
            "pom" => "application/xml".to_string(),
            "war" => "application/java-archive".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MavenGetError {
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(MavenValidationError),
    #[error("Failed to read artifact: {0}")]
    ReadFailed(MavenReadError),
}

pub struct HandleMavenPutArtifactUseCase {
    artifact_writer: Arc<dyn MavenArtifactWriter>,
}

impl HandleMavenPutArtifactUseCase {
    pub fn new(artifact_writer: Arc<dyn MavenArtifactWriter>) -> Self {
        Self { artifact_writer }
    }
    
    pub async fn execute(&self, request: MavenPutArtifactRequest) -> Result<MavenPutArtifactResponse, MavenPutError> {
        // Validar coordenadas
        request.coordinates.validate()
            .map_err(|e| MavenPutError::InvalidCoordinates(e))?;
        
        // Validar content type
        self.validate_content_type(&request.content_type)?;
        
        // Escribir artefacto
        self.artifact_writer.write_artifact(&request.coordinates, &request.content).await
            .map_err(|e| MavenPutError::WriteFailed(e))?;
        
        Ok(MavenPutArtifactResponse {
            success: true,
            message: format!("Artifact {} uploaded successfully", request.coordinates.to_path()),
        })
    }
    
    fn validate_content_type(&self, content_type: &str) -> Result<(), MavenPutError> {
        let valid_types = ["application/java-archive", "application/xml", "application/octet-stream"];
        if !valid_types.contains(&content_type) {
            return Err(MavenPutError::InvalidContentType(content_type.to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MavenPutError {
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(MavenValidationError),
    #[error("Invalid content type: {0}")]
    InvalidContentType(String),
    #[error("Failed to write artifact: {0}")]
    WriteFailed(MavenWriteError),
}
```

### Paso 3: Implementar Feature `handle_npm_request`

Similar estructura pero con puertos COMPLETAMENTE DIFERENTES:

```rust
// crates/distribution/src/features/handle_npm_request/src/ports.rs
#[async_trait]
pub trait NpmPackageReader {
    async fn read_package(&self, name: &str) -> Result<NpmPackage, NpmReadError>;
    async fn read_tarball(&self, name: &str, version: &str) -> Result<Vec<u8>, NpmReadError>;
}

#[async_trait]
pub trait NpmPackageWriter {
    async fn write_package(&self, package: &NpmPackage) -> Result<(), NpmWriteError>;
    async fn write_tarball(&self, name: &str, version: &str, content: &[u8]) -> Result<(), NpmWriteError>;
}
```

### Paso 4: Implementar Feature `handle_docker_request`

```rust
// crates/distribution/src/features/handle_docker_request/src/ports.rs
#[async_trait]
pub trait DockerManifestReader {
    async fn read_manifest(&self, name: &str, reference: &str) -> Result<DockerManifest, DockerReadError>;
}

#[async_trait]
pub trait DockerManifestWriter {
    async fn write_manifest(&self, name: &str, reference: &str, manifest: &DockerManifest) -> Result<(), DockerWriteError>;
}

#[async_trait]
pub trait DockerBlobReader {
    async fn read_blob(&self, name: &str, digest: &str) -> Result<Vec<u8>, DockerReadError>;
}

#[async_trait]
pub trait DockerBlobWriter {
    async fn write_blob(&self, name: &str, digest: &str, content: &[u8]) -> Result<(), DockerWriteError>;
}
```

### Paso 5: Crear Application Layer

```rust
// crates/distribution/src/application/src/format_orchestrator.rs
use std::sync::Arc;
use shared::enums::Ecosystem;

/// Orquesta features sin acoplar implementaciones
pub struct FormatOrchestrator {
    maven_get_handler: Arc<dyn crate::features::handle_maven_request::ports::MavenRequestHandler>,
    npm_get_handler: Arc<dyn crate::features::handle_npm_request::ports::NpmRequestHandler>,
    docker_get_handler: Arc<dyn crate::features::handle_docker_request::ports::DockerRequestHandler>,
}

impl FormatOrchestrator {
    pub async fn handle_request(&self, ecosystem: Ecosystem, path: &str, method: &str) -> Result<Vec<u8>, OrchestratorError> {
        match (ecosystem, method) {
            (Ecosystem::Maven, "GET") => {
                self.maven_get_handler.handle_get_artifact(path).await
            }
            (Ecosystem::Npm, "GET") => {
                self.npm_get_handler.handle_get_package(path).await
            }
            (Ecosystem::Docker, "GET") => {
                self.docker_get_handler.handle_get_manifest(path).await
            }
            _ => Err(OrchestratorError::UnsupportedOperation),
        }
    }
}
```

### Paso 6: Crear Infrastructure Layer

```rust
// crates/distribution/src/infrastructure/src/maven_storage.rs
use async_trait::async_trait;
use crate::features::handle_maven_request::ports::{MavenArtifactReader, MavenArtifactWriter, MavenReadError, MavenWriteError};

pub struct MavenS3Storage {
    s3_client: aws_sdk_s3::Client,
    bucket: String,
}

#[async_trait]
impl MavenArtifactReader for MavenS3Storage {
    async fn read_artifact(&self, coordinates: &MavenCoordinates) -> Result<Vec<u8>, MavenReadError> {
        let key = format!("maven/{}/{}", coordinates.group_id, coordinates.to_path());
        
        self.s3_client.get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| MavenReadError::StorageError(e.to_string()))?
            .body
            .collect()
            .await
            .map_err(|e| MavenReadError::StorageError(e.to_string()))
            .map(|data| data.to_vec())
    }
}
```

## 🧪 Estrategia de Testing por Feature

### Tests Unitarios por Feature
```bash
# Test específico de feature Maven
cargo test -p distribution --test handle_maven_request_test

# Test específico de feature npm  
cargo test -p distribution --test handle_npm_request_test

# Test específico de feature Docker
cargo test -p distribution --test handle_docker_request_test
```

### Tests de Integración
```rust
// crates/distribution/tests/it_maven_integration.rs
#[tokio::test]
async fn test_maven_artifact_upload_download() {
    // Arrange: Setup con mocks
    let container = HandleMavenRequestDIContainer::for_testing().await;
    
    // Act: Upload artifact
    let upload_request = MavenPutArtifactRequest {
        coordinates: MavenCoordinates::new("com.example", "test-app", "1.0.0").unwrap(),
        content: b"test jar content".to_vec(),
        content_type: "application/java-archive".to_string(),
    };
    
    let upload_result = container.api.handle_put(upload_request).await.unwrap();
    assert!(upload_result.success);
    
    // Act: Download artifact
    let download_request = MavenGetArtifactRequest {
        coordinates: MavenCoordinates::new("com.example", "test-app", "1.0.0").unwrap(),
    };
    
    let download_result = container.api.handle_get(download_request).await.unwrap();
    assert_eq!(download_result.content, b"test jar content");
}
```

## 📊 Métricas de Éxito

1. **✅ Independencia**: Cada feature compila y testea solo
2. **✅ Sin Dependencias Cruzadas**: Features no importan otros features
3. **✅ Puertos Segregados**: Interfaces específicas por feature
4. **✅ Dominio Puro**: Sin async/await en domain layer
5. **✅ Tests Aislados**: Mocks específicos por feature
6. **✅ Clean Architecture**: Separación clara de capas

## 🚀 Próximos Pasos

1. **Implementar feature Maven** completo con tests
2. **Implementar feature npm** completo con tests
3. **Implementar feature Docker** completo con tests
4. **Crear orchestrator** en application layer
5. **Integrar con API HTTP** manteniendo separación
6. **Ejecutar tests de integración** con clientes reales

Esta implementación garantiza una arquitectura VSA pura con Clean Architecture, donde cada feature es completamente independiente y sigue los principios SOLID.