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

## Arquitectura Detallada

### Excelencia Operacional
El crate `artifact` está diseñado para ser robusto y fácil de operar en producción:
- **Manejo de Errores Estructurado**: Utiliza `thiserror` para definir errores específicos (`UploadArtifactError`, `DomainError`), permitiendo un mapeo claro a códigos de estado HTTP (`api.rs`) y facilitando la depuración.
- **Resiliencia**: La arquitectura de puertos y adaptadores permite la simulación de fallos en dependencias externas (MongoDB, S3, RabbitMQ) mediante mocks (`test_adapter.rs`), asegurando que el sistema se comporte correctamente ante interrupciones.
- **Observabilidad (Logging y Tracing)**: La integración con la crate `tracing` (`api.rs`, `use_case.rs`, `test_adapter.rs`) proporciona logging estructurado y creación de spans (`info_span!`). Esto es fundamental para el monitoreo en tiempo real, el seguimiento de solicitudes y la identificación de cuellos de botella o errores en entornos de producción.

### Eficiencia y Rendimiento
El diseño del crate prioriza la eficiencia y el rendimiento:
- **Deduplicación de Artefactos Físicos**: Antes de almacenar un binario, el `use_case.rs` calcula un hash SHA256 y verifica si el artefacto físico ya existe en el repositorio. Esto evita el almacenamiento redundante de datos idénticos, optimizando el uso del espacio y reduciendo el tiempo de subida para contenido duplicado.
- **Operaciones Asíncronas**: El uso extensivo de `async/await` con `tokio` garantiza operaciones de I/O no bloqueantes, crucial para manejar un alto volumen de solicitudes concurrentes y mantener una alta capacidad de respuesta.
- **Almacenamiento Optimizado**: La integración con S3/MinIO (`adapter.rs`) proporciona una solución de almacenamiento de objetos escalable y de alto rendimiento, ideal para grandes volúmenes de datos binarios.
- **Manejo de Multipart Uploads**: El endpoint HTTP (`api.rs`) está optimizado para manejar `multipart/form-data`, lo que permite subidas eficientes de archivos grandes.

### Seguridad
La seguridad es un pilar fundamental en el diseño del crate:
- **Integridad del Contenido**: El cálculo y uso del hash SHA256 (`use_case.rs`) no solo sirve para la deduplicación, sino que también actúa como una verificación de integridad, asegurando que el artefacto no ha sido alterado.
- **Inmutabilidad de Artefactos Físicos**: Los `PhysicalArtifacts` son inmutables y su identidad se basa en su hash de contenido, lo que previene modificaciones no autorizadas de los binarios almacenados.
- **Control de Acceso Basado en Atributos (ABAC) con Cedar**: La integración con `cedar-policy` (`cedar_adapter.rs`) permite definir políticas de autorización granulares. `PackageVersion` expone atributos y relaciones jerárquicas para que Cedar pueda evaluar permisos de acceso de manera precisa.
- **Usuario de Sistema para Operaciones Internas**: Las operaciones de ciclo de vida son atribuidas a un `UserId::new_system_user()`, una buena práctica para auditar y segregar permisos de acciones automatizadas.

### Buenas Prácticas
El crate sigue rigurosamente principios de diseño de software modernos:
- **Vertical Slice Architecture (VSA)**: La organización del código en `src/features/upload_artifact` (con `api.rs`, `use_case.rs`, `adapter.rs`, `ports.rs`, etc.) encapsula verticalmente la funcionalidad de subida, mejorando la cohesión y la mantenibilidad.
- **Arquitectura Hexagonal (Ports & Adapters)**: La clara separación de interfaces (`ports.rs`) y sus implementaciones concretas (`adapter.rs`, `test_adapter.rs`) promueve el bajo acoplamiento, la testabilidad y la flexibilidad para cambiar implementaciones subyacentes.
- **Domain-Driven Design (DDD)**: El módulo `domain/` contiene las entidades centrales (`PackageVersion`, `PhysicalArtifact`) y eventos de dominio (`events.rs`), asegurando que la lógica de negocio sea el foco principal y esté bien modelada.
- **Inyección de Dependencias (DI)**: El módulo `di.rs` facilita la configuración y el cableado de las dependencias, tanto para entornos de producción como para pruebas, lo que simplifica la gestión de la complejidad y mejora la testabilidad.

### Monitorización
La visibilidad del sistema se logra a través de:
- **Integración con Tracing**: Los logs detallados y los spans generados por `tracing` permiten el seguimiento del flujo de ejecución de las solicitudes, la identificación de errores y el análisis de rendimiento. Esto es crucial para integrar con herramientas de APM y sistemas de monitoreo centralizados.
- **Publicación de Eventos de Dominio**: La emisión de `ArtifactEvent`s a través de RabbitMQ proporciona un flujo de eventos de negocio que pueden ser consumidos por sistemas de auditoría, análisis de datos o dashboards de monitoreo para obtener información en tiempo real sobre la actividad del repositorio.

## Tests

Este crate incluye tanto tests unitarios como tests de integración end-to-end, diseñados para asegurar la calidad y robustez del código desde múltiples perspectivas.

### Tests Unitarios

Los tests unitarios usan mocks y no requieren servicios externos, lo que los hace rápidos y fiables:

```bash
# Ejecutar todos los tests unitarios
cargo test --lib -p artifact

# Ejecutar tests de un módulo específico
cargo test -p artifact use_case_test

# Con logs detallados
RUST_LOG=debug cargo test --lib -p artifact -- --nocapture
```

**Ubicación**: `src/features/upload_artifact/use_case_test.rs` y `src/features/upload_artifact/api_test.rs`

**Contribución a la Arquitectura**:
- **Correctitud y Fiabilidad**: Verifican la lógica de negocio central (`use_case.rs`) y el comportamiento de la API (`api.rs`) en aislamiento, asegurando que cada componente funcione según lo esperado.
- **Eficiencia en el Desarrollo**: Proporcionan un ciclo de retroalimentación rápido para los desarrolladores, permitiendo detectar y corregir errores tempranamente.
- **Mantenibilidad**: Al usar mocks (`test_adapter.rs`), garantizan que los cambios en un componente no rompan inesperadamente otros, facilitando refactorizaciones seguras.
- **Verificación de Observabilidad**: El uso de `traced_test` permite verificar que los logs y spans se generan correctamente, asegurando que la instrumentación de `tracing` sea efectiva para la monitorización.

**Cobertura**:
- ✅ Upload de nuevo artefacto (creación completa)
- ✅ Deduplicación de artefactos existentes
- ✅ Validación de comandos
- ✅ Manejo de errores
- ✅ Comportamiento de la API HTTP (respuestas, códigos de estado)
- ✅ Verificación de logs y spans generados

### Tests de Integración

Los tests de integración usan contenedores Docker reales (`testcontainers`) para simular un entorno cercano a producción y están protegidos por la feature `integration`.

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

**Ubicación**: `tests/it_upload_artifact.rs`, `tests/it_mongodb_isolated.rs`, `tests/it_testcontainers_isolated.rs`

**Infraestructura**: Cada test levanta contenedores Docker con:
- MongoDB 6.0
- MinIO (S3-compatible)  
- RabbitMQ 3.13

**Contribución a la Arquitectura**:
- **Excelencia Operacional**: Validan la interacción con dependencias externas reales, asegurando que el sistema funcione correctamente en un entorno integrado, lo que reduce los riesgos de despliegue.
- **Rendimiento y Eficiencia**: Las pruebas de concurrencia y manejo de archivos grandes (`it_upload_artifact.rs`) validan directamente las características de rendimiento y escalabilidad del sistema.
- **Resiliencia**: Al usar contenedores reales, se verifica la capacidad del sistema para interactuar y recuperarse de posibles problemas con los servicios externos.
- **Seguridad**: Indirectamente, validan el flujo de datos y la integridad con el almacenamiento real.
- **Validación de la Arquitectura Hexagonal**: Confirman que los adaptadores de producción se integran correctamente con los puertos y el caso de uso.

**Cobertura**:
- ✅ **Upload básico end-to-end** - HTTP → MongoDB + S3 + Events
- ✅ **Deduplicación** - Mismo contenido, diferentes versiones
- ✅ **Validación HTTP** - Metadata faltante, archivo faltante, JSON inválido
- ✅ **Archivos grandes** - Upload de 5MB
- ✅ **Múltiples artefactos** - 3 uploads diferentes
- ✅ **Concurrencia** - 5 uploads simultáneos del mismo contenido
- ✅ **Conectividad de Infraestructura** - Pruebas aisladas para MongoDB y contenedores genéricos.

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
