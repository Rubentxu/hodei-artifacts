# Artifact Crate

Crate para la gestión de artefactos binarios: subida, descarga, metadatos, idempotencia y publicación de eventos.

## Arquitectura

Este crate implementa:
- **Upload de artefactos** con multipart/form-data
- **Deduplicación de artefactos físicos** basada en hash SHA256
- **Almacenamiento en S3/MinIO** para binarios
- **Persistencia en MongoDB** para metadatos
- **Publicación de eventos** via RabbitMQ
- **Arquitectura Hexagonal** con ports y adapters

## Estructura

```
src/
  features/
    upload_artifact/        # Feature completo de upload
      api.rs               # Endpoint HTTP (Axum)
      use_case.rs          # Lógica de negocio
      adapter.rs           # Implementaciones reales (S3, MongoDB, RabbitMQ)
      test_adapter.rs      # Mocks para testing
      ports.rs             # Traits/interfaces
      dto.rs               # Request/Response DTOs
      error.rs             # Errores específicos del feature
      di.rs                # Dependency Injection container
      use_case_test.rs     # Tests unitarios del use case
  domain/                  # Entidades de dominio
tests/                     # Tests de integración
  it_upload_artifact.rs    # Tests end-to-end completos
  it_mongodb_isolated.rs   # Tests aislados de MongoDB
  it_testcontainers_isolated.rs  # Tests básicos de containers
```

## Tests

Este crate incluye tanto tests unitarios como tests de integración end-to-end.

### Tests Unitarios

Los tests unitarios usan mocks y no requieren servicios externos:

```bash
# Ejecutar todos los tests unitarios
cargo test --lib -p artifact

# Ejecutar tests de un módulo específico
cargo test -p artifact use_case_test

# Con logs detallados
RUST_LOG=debug cargo test --lib -p artifact -- --nocapture
```

**Ubicación**: `src/features/upload_artifact/use_case_test.rs`

**Cobertura**:
- ✅ Upload de nuevo artefacto (creación completa)
- ✅ Deduplicación de artefactos existentes
- ✅ Validación de comandos
- ✅ Manejo de errores

### Tests de Integración

Los tests de integración usan contenedores Docker reales y están protegidos por la feature `integration`.

```bash
# Verificar que compilan sin ejecutar
cargo test -p artifact --features integration --no-run

# Ejecutar todos los tests de integración (requiere Docker)
cargo test -p artifact --features integration -- --ignored

# Ejecutar un test específico
cargo test -p artifact --features integration test_upload_new_artifact_integration -- --ignored

# Con logs de testcontainers
RUST_LOG=testcontainers=debug,artifact=info cargo test -p artifact --features integration -- --ignored --nocapture
```

**Ubicación**: `tests/it_upload_artifact.rs`

**Infraestructura**: Cada test levanta contenedores Docker con:
- MongoDB 6.0
- MinIO (S3-compatible)  
- RabbitMQ 3.13

**Cobertura**:
- ✅ **Upload básico end-to-end** - HTTP → MongoDB + S3 + Events
- ✅ **Deduplicación** - Mismo contenido, diferentes versiones
- ✅ **Validación HTTP** - Metadata faltante, archivo faltante, JSON inválido
- ✅ **Archivos grandes** - Upload de 5MB
- ✅ **Múltiples artefactos** - 3 uploads diferentes
- ✅ **Concurrencia** - 5 uploads simultáneos del mismo contenido

### Tests de Infraestructura

Tests aislados para verificar conectividad básica:

```bash
# Test de conexión MongoDB
cargo test -p artifact --features integration test_mongodb_isolated_connection -- --ignored

# Test básico de contenedores
cargo test -p artifact --features integration test_hello_world_container -- --ignored
```

## Configuración de Docker para Tests

### Requisitos

- Docker Desktop o Docker Engine
- Suficientes recursos (2GB+ RAM para contenedores)
- Puerto dinámico disponible

### Solución de problemas en Linux

Si experimentas timeouts con testcontainers en distribuciones como Deepin/Ubuntu:

1. **Verificar que Docker funciona**:
   ```bash
   docker ps
   docker run hello-world
   ```

2. **Para sistemas con Podman**: Configurar testcontainers para usar Podman:
   ```bash
   # Habilitar API de Podman
   podman system service --time=0 &
   
   # Crear ~/.testcontainers.properties
   echo "docker.host=unix://${XDG_RUNTIME_DIR}/podman/podman.sock" > ~/.testcontainers.properties
   echo "ryuk.container.privileged=true" >> ~/.testcontainers.properties
   ```

3. **Verificar permisos**:
   ```bash
   sudo usermod -aG docker $USER
   # Reiniciar sesión
   ```

## Desarrollo

### Agregar nuevos tests

1. **Tests unitarios**: Añadir en `use_case_test.rs` usando los mocks
2. **Tests de integración**: Añadir en `it_upload_artifact.rs` con `#[ignore]`

### Estructura de test de integración

```rust
#[tokio::test]
#[ignore]  // No ejecutar por defecto
async fn test_mi_nuevo_caso() {
    let context = setup_test_environment().await;  // Infraestructura completa
    
    // Preparar datos
    let form = Form::new()...;
    
    // Ejecutar
    let response = context.http_client.post(...).send().await.unwrap();
    
    // Verificar HTTP
    assert_eq!(response.status(), StatusCode::CREATED);
    
    // Verificar persistencia MongoDB
    let db = context.mongo_client.database("test_db");
    // ... verificaciones de BD
}
```

## Dependencies

- **Runtime**: `axum`, `mongodb`, `aws-sdk-s3`, `lapin`, `bytes`, `serde`
- **Testing**: `reqwest`, `testcontainers`, `tokio-test`
- **Features**: `integration` (para tests de integración)

Ver `Cargo.toml` para versiones específicas.
