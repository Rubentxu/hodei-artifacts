# Plan de Reestructuración VSA para Distribution Crate

## 📋 Resumen

Este documento detalla el plan para migrar la implementación actual de manejadores de formato (Maven, npm, Docker) a una arquitectura pura de Vertical Slice Architecture (VSA) con Clean Architecture, siguiendo los principios de segregación de interfaces y dominio puro.

## 🚨 Problemas con la Implementación Actual

La implementación actual viola varios principios fundamentales de VSA:

1. **❌ Servicios Monolíticos**: `MavenFormatHandler`, `NpmFormatHandler`, `DockerFormatHandler` contienen toda la lógica
2. **❌ Interfaces Compartidas**: Todos usan el mismo `FormatHandler` trait
3. **❌ Dependencias de Infraestructura**: Lógica de negocio mezclada con detalles técnicos
4. **❌ Sin Segregación de Interfaces**: Puertos genéricos en lugar de específicos por feature
5. **❌ Acoplamiento Fuerte**: Features dependen de implementaciones concretas

## ✅ Principios VSA/Clean Architecture a Implementar

### 1. **Segregación de Interfaces (ISP)**
```rust
// ❌ ANTI-PATRÓN: Interfaz genérica compartida
trait FormatHandler {
    fn handle_request(&self, request: FormatRequest) -> Result<FormatResponse>;
    fn can_handle(&self, path: &str, ecosystem: &Ecosystem) -> bool;
}

// ✅ PATRÓN VSA: Interfaces específicas por feature
// Feature: handle_maven_request
trait MavenRequestHandler {
    fn handle_maven_get(&self, path: &str) -> Result<MavenArtifactResponse>;
    fn handle_maven_put(&self, path: &str, content: &[u8]) -> Result<MavenUploadResponse>;
}

// Feature: handle_npm_request  
trait NpmRequestHandler {
    fn handle_npm_get(&self, package_name: &str) -> Result<NpmPackageResponse>;
    fn handle_npm_put(&self, package_name: &str, tarball: &[u8]) -> Result<NpmPublishResponse>;
}
```

### 2. **Dominio Puro**
```rust
// ✅ Dominio sin dependencias de infraestructura
pub struct MavenCoordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenCoordinates {
    pub fn validate(&self) -> Result<(), MavenValidationError> {
        // Lógica pura de validación
    }
}
```

### 3. **Features con Puertos Propios**
Cada feature define SUS PROPIAS interfaces, incluso si son similares a otras features.

## 🏗️ Nueva Estructura VSA

### Estructura de Directorios

```
crates/distribution/
├── src/
│   ├── features/
│   │   ├── handle_maven_request/
│   │   │   ├── mod.rs              # Exportaciones públicas
│   │   │   ├── dto.rs              # DTOs específicos del feature
│   │   │   ├── ports.rs            # Interfaces SEGREGADAS
│   │   │   ├── use_case.rs         # Lógica del caso de uso
│   │   │   ├── adapter.rs          # Implementaciones concretas
│   │   │   ├── api.rs              # Punto de entrada HTTP
│   │   │   └── di.rs               # Configuración DI
│   │   ├── handle_npm_request/
│   │   │   └── ...                 # Misma estructura con PROPIOS ports
│   │   ├── handle_docker_request/
│   │   │   └── ...                 # Misma estructura con PROPIOS ports
│   │   ├── generate_maven_metadata/
│   │   │   └── ...                 # Feature separado para metadata
│   │   ├── generate_npm_metadata/
│   │   │   └── ...                 # Feature separado para metadata
│   │   └── generate_docker_manifest/
│   │       └── ...                 # Feature separado para manifests
│   ├── domain/
│   │   ├── maven/                  # Dominio Maven puro
│   │   │   ├── coordinates.rs      # Value Objects
│   │   │   ├── metadata.rs         # Entidades de metadata
│   │   │   └── validation.rs       # Reglas de negocio
│   │   ├── npm/                    # Dominio npm puro
│   │   └── docker/                 # Dominio Docker puro
│   ├── application/                # Orquestación de features
│   └── infrastructure/             # Implementaciones técnicas
```

## 📋 Features Específicos a Implementar

### 1. **Feature: `handle_maven_request`**
**Responsabilidad**: Procesar requests HTTP Maven (GET/PUT)

```rust
// ports.rs - Interfaces SEGREGADAS para este feature
pub trait MavenRequestHandler {
    fn handle_get_artifact(&self, coordinates: &MavenCoordinates) -> Result<Vec<u8>, MavenError>;
    fn handle_put_artifact(&self, coordinates: &MavenCoordinates, content: &[u8]) -> Result<(), MavenError>;
}

pub trait MavenMetadataGenerator {
    fn generate_metadata(&self, group_id: &str, artifact_id: &str) -> Result<MavenMetadata, MavenError>;
}
```

### 2. **Feature: `handle_npm_request`**
**Responsabilidad**: Procesar requests HTTP npm (GET/PUT)

```rust
// ports.rs - Interfaces SEGREGADAS para este feature  
pub trait NpmRequestHandler {
    fn handle_get_package(&self, name: &str) -> Result<NpmPackage, NpmError>;
    fn handle_put_package(&self, name: &str, tarball: &[u8]) -> Result<(), NpmError>;
}

pub trait NpmDistTagManager {
    fn get_dist_tags(&self, name: &str) -> Result<HashMap<String, String>, NpmError>;
    fn update_dist_tag(&self, name: &str, tag: &str, version: &str) -> Result<(), NpmError>;
}
```

### 3. **Feature: `handle_docker_request`**
**Responsabilidad**: Procesar requests Docker Registry V2 API

```rust
// ports.rs - Interfaces SEGREGADAS para este feature
pub trait DockerRequestHandler {
    fn handle_get_manifest(&self, name: &str, reference: &str) -> Result<DockerManifest, DockerError>;
    fn handle_put_manifest(&self, name: &str, reference: &str, manifest: &DockerManifest) -> Result<(), DockerError>;
    fn handle_get_blob(&self, name: &str, digest: &str) -> Result<Vec<u8>, DockerError>;
    fn handle_get_catalog(&self) -> Result<Vec<String>, DockerError>;
    fn handle_get_tags(&self, name: &str) -> Result<Vec<String>, DockerError>;
}
```

### 4. **Feature: `generate_maven_metadata`**
**Responsabilidad**: Generar maven-metadata.xml dinámico

```rust
// ports.rs - Interfaces SEGREGADAS para este feature
pub trait MavenMetadataRepository {
    fn get_versions(&self, group_id: &str, artifact_id: &str) -> Result<Vec<String>, MavenError>;
    fn get_latest_version(&self, group_id: &str, artifact_id: &str) -> Result<Option<String>, MavenError>;
}

pub trait MavenMetadataGenerator {
    fn generate_metadata_xml(&self, group_id: &str, artifact_id: &str) -> Result<String, MavenError>;
}
```

## 🔧 Principios de Implementación

### 1. **Sin Dependencias Cruzadas entre Features**
```rust
// ❌ PROHIBIDO: Un feature importando otro feature
use crate::features::handle_npm_request::dto::NpmPackage; // ❌

// ✅ PERMITIDO: Solo importar desde domain o shared
use crate::domain::maven::coordinates::MavenCoordinates; // ✅
use shared::hrn::RepositoryId; // ✅
```

### 2. **Puertos Específicos por Feature**
Cada feature define sus propios puertos, incluso si son similares:
```rust
// Feature Maven
trait MavenArtifactReader { fn read(&self, id: &str) -> Result<Vec<u8>, Error>; }

// Feature npm  
trait NpmPackageReader { fn read(&self, name: &str) -> Result<Vec<u8>, Error>; }

// Feature Docker
trait DockerBlobReader { fn read(&self, digest: &str) -> Result<Vec<u8>, Error>; }
```

### 3. **Dominio Puro sin Async**
```rust
// ✅ Dominio síncrono y puro
pub struct MavenMetadata {
    pub group_id: String,
    pub artifact_id: String,
    pub versions: Vec<String>,
}

impl MavenMetadata {
    pub fn get_latest_version(&self) -> Option<&str> {
        self.versions.last()
    }
}
```

### 4. **Use Cases con Lógica Específica**
```rust
pub struct HandleMavenGetArtifactUseCase {
    artifact_reader: Arc<dyn MavenArtifactReader>,
    metadata_generator: Arc<dyn MavenMetadataGenerator>,
}

impl HandleMavenGetArtifactUseCase {
    pub async fn execute(&self, coordinates: MavenCoordinates) -> Result<Vec<u8>, MavenError> {
        // Validar coordenadas
        coordinates.validate()?;
        
        // Leer artefacto
        let content = self.artifact_reader.read(&coordinates.to_string()).await?;
        
        // Generar metadata si es necesario
        if coordinates.is_metadata_request() {
            let metadata = self.metadata_generator.generate_metadata_xml(
                &coordinates.group_id,
                &coordinates.artifact_id
            ).await?;
            // ... lógica específica
        }
        
        Ok(content)
    }
}
```

## 🧪 Estrategia de Testing

### Tests por Feature
```rust
// crates/distribution/src/features/handle_maven_request/use_case_test.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::handle_maven_request::ports::test::MockMavenArtifactReader;
    
    #[tokio::test]
    async fn test_handle_valid_maven_coordinates() {
        // Arrange
        let mock_reader = Arc::new(MockMavenArtifactReader::new());
        let use_case = HandleMavenGetArtifactUseCase::new(mock_reader);
        let coordinates = MavenCoordinates::new("com.example", "my-app", "1.0.0");
        
        // Act
        let result = use_case.execute(coordinates).await;
        
        // Assert
        assert!(result.is_ok());
    }
}
```

## 📊 Beneficios de esta Arquitectura

1. **🔒 Aislamiento Total**: Cada feature es independiente
2. **🔄 Sustitución Fácil**: Implementaciones intercambiables por feature
3. **🧪 Testing Simplificado**: Mocks específicos por feature
4. **📈 Escalabilidad**: Nuevas features sin afectar existentes
5. **🎯 SOLID**: Principios ISP, DIP, SRP aplicados
6. **🏗️ Clean Architecture**: Separación clara de capas

## 🚀 Próximos Pasos

1. **Crear estructura base** con directorios de features
2. **Migrar dominio Maven** a estructura VSA
3. **Migrar dominio npm** a estructura VSA  
4. **Migrar dominio Docker** a estructura VSA
5. **Implementar features** con puertos segregados
6. **Crear tests** para cada feature independiente
7. **Integrar con API HTTP** manteniendo separación

Esta reestructura garantiza que cada feature sea completamente independiente, siguiendo los principios de VSA y Clean Architecture con segregación estricta de interfaces.