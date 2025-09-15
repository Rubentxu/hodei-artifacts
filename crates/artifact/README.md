# Artifact Crate

Crate para la gestión de artefactos binarios: subida, descarga, metadatos, idempotencia y publicación de eventos.

## 1. Visión General

El crate `artifact` es un componente central de Hodei Artifacts, diseñado para manejar de forma robusta y eficiente el
ciclo de vida de los artefactos binarios. Sigue rigurosamente los principios de la **Arquitectura de Slice Vertical (
VSA)** y la **Arquitectura Hexagonal (Ports & Adapters)**, lo que lo convierte en un ejemplo claro de cómo estructurar
nuevas funcionalidades en el proyecto.

### Funcionalidades Clave

Este crate implementa:

- **Upload de artefactos** con `multipart/form-data` para subidas eficientes.
- **Deduplicación de artefactos físicos** basada en hash SHA256 para optimizar el almacenamiento.
- **Almacenamiento en S3/MinIO** para los binarios, garantizando escalabilidad y durabilidad.
- **Persistencia en MongoDB** para los metadatos de los artefactos.
- **Publicación de eventos** vía RabbitMQ para una integración desacoplada con otros servicios.

### Estructura del Código

La organización del código refleja la VSA, con cada funcionalidad principal encapsulada en su propio "slice". La
característica `upload_artifact` es un ejemplo completo de esta estructura:

```
src/
  features/
    upload_artifact/        # Feature completo de upload (Vertical Slice)
      api.rs               # Endpoint HTTP (Axum) - Capa de Entrada
      use_case.rs          # Lógica de negocio - Núcleo del Dominio
      adapter.rs           # Implementaciones reales (S3, MongoDB, RabbitMQ) - Adaptadores de Salida
      test_adapter.rs      # Mocks para testing - Adaptadores de Prueba
      ports.rs             # Traits/interfaces - Puertos de Salida
      dto.rs               # Request/Response DTOs - Contratos de Datos
      error.rs             # Errores específicos del feature
      di.rs                # Dependency Injection container - Configuración de Dependencias
      use_case_test.rs     # Tests unitarios del use case
      api_test.rs          # Tests unitarios del API
  domain/                  # Entidades de dominio y eventos compartidos
  infrastructure/          # Adaptadores de infraestructura compartidos (ej. Cedar)
tests/                     # Tests de integración end-to-end
  it_upload_artifact.rs    # Tests end-to-end completos
  it_mongodb_isolated.rs   # Tests aislados de conectividad MongoDB
  it_testcontainers_isolated.rs  # Tests básicos de contenedores
```

## 2. Principios Arquitectónicos y Excelencia

El diseño del crate `artifact` se basa en principios sólidos para garantizar un sistema robusto, eficiente, seguro y
fácil de mantener y monitorear.

### 2.1. Excelencia Operacional

El crate `artifact` está diseñado para ser robusto y fácil de operar en producción, minimizando la intervención manual y
facilitando la resolución de problemas:

- **Manejo de Errores Estructurado**: Utiliza la crate `thiserror` para definir errores específicos (
  `UploadArtifactError`, `DomainError`). Esto permite un mapeo claro a códigos de estado HTTP (`api.rs`) y facilita la
  depuración y el diagnóstico de problemas en producción. Los errores son informativos y contextuales.
- **Resiliencia y Tolerancia a Fallos**: La arquitectura de puertos y adaptadores permite la simulación de fallos en
  dependencias externas (MongoDB, S3, RabbitMQ) mediante mocks (`test_adapter.rs`). Esto asegura que el sistema se
  comporte correctamente y degrade de forma controlada ante interrupciones o latencias en servicios externos.
- **Observabilidad (Logging y Tracing)**: La integración profunda con la crate `tracing` (`api.rs`, `use_case.rs`,
  `test_adapter.rs`) proporciona logging estructurado y la creación de spans (`info_span!`). Esto es fundamental para el
  monitoreo en tiempo real, el seguimiento de solicitudes a través de los componentes y la identificación rápida de
  cuellos de botella o errores en entornos de producción.

### 2.2. Eficiencia y Rendimiento

El diseño del crate prioriza la eficiencia y el rendimiento para manejar grandes volúmenes de artefactos y solicitudes:

- **Deduplicación de Artefactos Físicos**: Antes de almacenar un binario, el `use_case.rs` calcula un hash SHA256 del
  contenido. Si un artefacto físico con el mismo hash ya existe en el repositorio, se reutiliza la referencia al
  artefacto existente en lugar de almacenar una nueva copia. Esto evita el almacenamiento redundante de datos idénticos,
  optimizando el uso del espacio y reduciendo el tiempo de subida para contenido duplicado.
- **Operaciones Asíncronas**: El uso extensivo de `async/await` con `tokio` garantiza operaciones de I/O no bloqueantes.
  Esto es crucial para manejar un alto volumen de solicitudes concurrentes y mantener una alta capacidad de respuesta,
  ya que el hilo de ejecución no se bloquea esperando operaciones de red o disco.
- **Almacenamiento Optimizado**: La integración con S3/MinIO (`adapter.rs`) proporciona una solución de almacenamiento
  de objetos escalable, de bajo costo y de alto rendimiento, ideal para grandes volúmenes de datos binarios.
- **Manejo de Multipart Uploads**: El endpoint HTTP (`api.rs`) está optimizado para manejar `multipart/form-data`, lo
  que permite subidas eficientes de archivos grandes al dividir el contenido en partes.

### 2.3. Seguridad

La seguridad es un pilar fundamental en el diseño del crate, con medidas integradas para proteger la integridad y el
acceso a los artefactos:

- **Integridad del Contenido**: El cálculo y uso del hash SHA256 (`use_case.rs`) no solo sirve para la deduplicación,
  sino que también actúa como una verificación de integridad criptográfica. Esto asegura que el artefacto no ha sido
  alterado desde su subida y que el contenido recuperado es idéntico al original.
- **Inmutabilidad de Artefactos Físicos**: Los `PhysicalArtifacts` son inmutables una vez almacenados, y su identidad se
  basa en su hash de contenido. Esto previene modificaciones no autorizadas de los binarios almacenados, garantizando la
  confianza en los artefactos.
- **Control de Acceso Basado en Atributos (ABAC) con Cedar**: La integración con `cedar-policy` (`cedar_adapter.rs`)
  permite definir políticas de autorización granulares y dinámicas. `PackageVersion` expone atributos y relaciones
  jerárquicas (organización, repositorio) para que Cedar pueda evaluar permisos de acceso de manera precisa y flexible.
- **Usuario de Sistema para Operaciones Internas**: Las operaciones de ciclo de vida y otras acciones automatizadas son
  atribuidas a un `UserId::new_system_user()`. Esta es una buena práctica para auditar y segregar permisos de acciones
  realizadas por el sistema versus las realizadas por usuarios humanos.

### 2.4. Buenas Prácticas de Diseño

El crate sigue rigurosamente principios de diseño de software modernos para garantizar un código limpio, mantenible y
extensible:

- **Vertical Slice Architecture (VSA)**: La organización del código en `src/features/upload_artifact` (con `api.rs`,
  `use_case.rs`, `adapter.rs`, `ports.rs`, etc.) encapsula verticalmente toda la funcionalidad relacionada con la subida
  de artefactos. Esto mejora la cohesión, reduce el acoplamiento entre features y facilita el entendimiento y la
  evolución del código.
- **Arquitectura Hexagonal (Ports & Adapters)**: La clara separación de interfaces (`ports.rs`) y sus implementaciones
  concretas (`adapter.rs` para producción, `test_adapter.rs` para pruebas) promueve el bajo acoplamiento, la
  testabilidad y la flexibilidad. Permite cambiar implementaciones subyacentes (ej. de MongoDB a otra DB) sin afectar la
  lógica de negocio.
- **Domain-Driven Design (DDD)**: El módulo `domain/` contiene las entidades centrales (`PackageVersion`,
  `PhysicalArtifact`) y eventos de dominio (`events.rs`). Esto asegura que la lógica de negocio sea el foco principal,
  esté bien modelada y sea independiente de los detalles de infraestructura.
- **Inyección de Dependencias (DI)**: El módulo `di.rs` facilita la configuración y el "cableado" de las dependencias.
  Permite que los componentes reciban sus dependencias a través de sus constructores, en lugar de crearlas internamente,
  lo que es fundamental para la Inversión de Control (IoC) y la testabilidad.

### 2.5. Observabilidad y Monitorización

La visibilidad del sistema se logra a través de una instrumentación cuidadosa que permite el monitoreo proactivo y la
depuración eficiente:

- **Integración con Tracing**: Los logs detallados y los spans generados por `tracing` permiten el seguimiento del flujo
  de ejecución de las solicitudes a través de los diferentes componentes del sistema. Esto es crucial para identificar
  cuellos de botella, diagnosticar errores y analizar el rendimiento en entornos de producción. La información de los
  spans puede ser exportada a herramientas de APM (Application Performance Monitoring) para una visión centralizada.
- **Publicación de Eventos de Dominio**: La emisión de `ArtifactEvent`s (ej. `PackageVersionPublished`) a través de
  RabbitMQ proporciona un flujo de eventos de negocio. Estos eventos pueden ser consumidos por sistemas de auditoría,
  análisis de datos o dashboards de monitoreo para obtener información en tiempo real sobre la actividad del
  repositorio, el uso de artefactos y el estado del sistema.

## 3. Estrategia de Testing

Este crate incluye tanto tests unitarios como tests de integración end-to-end, diseñados para asegurar la calidad y
robustez del código desde múltiples perspectivas y servir como un modelo para otras features.

### 3.1. Tests Unitarios

Los tests unitarios se centran en la lógica de negocio y el comportamiento de la API en aislamiento, usando mocks para
las dependencias externas. Son rápidos, fiables y proporcionan un feedback inmediato durante el desarrollo.

```bash
# Ejecutar todos los tests unitarios
cargo test --lib -p artifact

# Ejecutar tests de un módulo específico (ej. use_case_test)
cargo test -p artifact use_case_test

# Con logs detallados
RUST_LOG=debug cargo test --lib -p artifact -- --nocapture
```

**Ubicación**: `src/features/upload_artifact/use_case_test.rs` y `src/features/upload_artifact/api_test.rs`

**Contribución a la Arquitectura**:

- **Correctitud y Fiabilidad**: Verifican que la lógica de negocio central (`use_case.rs`) y el comportamiento de la
  API (`api.rs`) funcionen según lo esperado en aislamiento.
- **Eficiencia en el Desarrollo**: Proporcionan un ciclo de retroalimentación rápido para los desarrolladores,
  permitiendo detectar y corregir errores tempranamente sin necesidad de levantar infraestructura externa.
- **Mantenibilidad**: Al usar mocks (`test_adapter.rs`), garantizan que los cambios en un componente no rompan
  inesperadamente otros, facilitando refactorizaciones seguras y el desarrollo paralelo.
- **Verificación de Observabilidad**: El uso de `traced_test` y `assert_log_contains!` permite verificar que los logs y
  spans se generan correctamente, asegurando que la instrumentación de `tracing` sea efectiva para la monitorización en
  producción.

**Cobertura**:

- ✅ Lógica de creación y deduplicación de artefactos.
- ✅ Validación de comandos de entrada.
- ✅ Manejo de errores en la lógica de negocio.
- ✅ Comportamiento de la API HTTP (respuestas, códigos de estado).
- ✅ Verificación de logs y spans generados por la instrumentación.

### 3.2. Tests de Integración

Los tests de integración validan la interacción del crate con dependencias externas reales, simulando un entorno cercano
a producción. Son cruciales para asegurar la fiabilidad del sistema en su conjunto.

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

**Infraestructura**: Cada test levanta contenedores Docker reales (`testcontainers`) con:

- MongoDB 6.0
- MinIO (S3-compatible)
- RabbitMQ 3.13

**Contribución a la Arquitectura**:

- **Excelencia Operacional**: Validan la interacción con dependencias externas reales, asegurando que el sistema
  funcione correctamente en un entorno integrado, lo que reduce los riesgos de despliegue y los problemas en producción.
- **Rendimiento y Eficiencia**: Las pruebas de concurrencia y manejo de archivos grandes (`it_upload_artifact.rs`)
  validan directamente las características de rendimiento y escalabilidad del sistema bajo cargas realistas.
- **Resiliencia**: Al usar contenedores reales, se verifica la capacidad del sistema para interactuar y recuperarse de
  posibles problemas (ej. desconexiones temporales) con los servicios externos.
- **Seguridad**: Indirectamente, validan el flujo de datos y la integridad con el almacenamiento real y los sistemas de
  mensajería.
- **Validación de la Arquitectura Hexagonal**: Confirman que los adaptadores de producción se integran correctamente con
  los puertos y el caso de uso, validando la implementación de los límites arquitectónicos.

**Cobertura**:

- ✅ **Flujo completo end-to-end**: Desde la recepción HTTP hasta la persistencia en MongoDB, almacenamiento en S3 y
  publicación de eventos en RabbitMQ.
- ✅ **Deduplicación**: Verificación de que la lógica de deduplicación funciona correctamente con contenido idéntico.
- ✅ **Validación HTTP**: Pruebas de escenarios con metadata o archivos faltantes, y JSON inválido.
- ✅ **Archivos grandes**: Subida de archivos de 5MB para verificar el rendimiento.
- ✅ **Múltiples artefactos**: Subida de varios artefactos diferentes en una sola ejecución.
- ✅ **Concurrencia**: Pruebas de 5 subidas simultáneas del mismo contenido para evaluar el manejo de la concurrencia.
- ✅ **Conectividad de Infraestructura**: Pruebas aisladas para verificar la conectividad básica con MongoDB y el
  funcionamiento de los contenedores genéricos.

## 4. Entorno de Desarrollo y Pruebas

Esta sección proporciona guías prácticas para configurar el entorno y contribuir con nuevos tests.

### 4.1. Configuración de Docker para Tests

Los tests de integración dependen de Docker para levantar los servicios externos.

#### Requisitos

- Docker Desktop o Docker Engine instalado y en ejecución.
- Suficientes recursos del sistema (se recomiendan 2GB+ de RAM para los contenedores de prueba).
- Puertos dinámicos disponibles para la asignación de `testcontainers`.

#### Solución de problemas en Linux

Si experimentas timeouts o problemas con `testcontainers` en distribuciones Linux (ej. Deepin/Ubuntu):

1. **Verificar que Docker funciona correctamente**:
   ```bash
   docker ps
   docker run hello-world
   ```

2. **Para sistemas con Podman**: Si utilizas Podman en lugar de Docker, configura `testcontainers` para usar la API de
   Podman:
   ```bash
   # Habilitar la API de Podman (se ejecutará en segundo plano)
   podman system service --time=0 &
   
   # Crear el archivo de configuración de testcontainers
   echo "docker.host=unix://${XDG_RUNTIME_DIR}/podman/podman.sock" > ~/.testcontainers.properties
   echo "ryuk.container.privileged=true" >> ~/.testcontainers.properties
   ```

3. **Verificar permisos de usuario**: Asegúrate de que tu usuario tenga los permisos adecuados para interactuar con
   Docker:
   ```bash
   sudo usermod -aG docker $USER
   # Después de ejecutar este comando, es necesario reiniciar la sesión para que los cambios surtan efecto.
   ```

### 4.2. Desarrollo de Tests

#### Agregar nuevos tests

- **Tests unitarios**: Añadir en `src/features/upload_artifact/use_case_test.rs` o
  `src/features/upload_artifact/api_test.rs` utilizando los mocks definidos en `test_adapter.rs`.
- **Tests de integración**: Añadir en `tests/it_upload_artifact.rs` (o crear nuevos archivos `it_*.rs` para suites de
  tests de integración específicas) y marcarlos con `#[ignore]` para que no se ejecuten por defecto con `cargo test`.

#### Estructura de un test de integración

```rust
#[tokio::test]
#[ignore]  // No ejecutar por defecto con `cargo test`
async fn test_mi_nuevo_caso() {
    // 1. Configuración del Entorno de Prueba
    // `setup_test_environment()` levanta todos los contenedores Docker necesarios (MongoDB, MinIO, RabbitMQ)
    // y configura los clientes HTTP, MongoDB, S3 y RabbitMQ, además de la inyección de dependencias.
    let context = setup_test_environment().await;
    
    // 2. Preparación de Datos
    // Crear los datos de entrada para la prueba, como el formulario multipart para la subida.
    let form = Form::new()...;
    
    // 3. Ejecución de la Lógica
    // Realizar la llamada HTTP al endpoint de la aplicación en prueba.
    let response = context.http_client.post(format!("{}/artifacts", context.app_url)).multipart(form).send().await.unwrap();
    
    // 4. Verificación de Resultados (Capa HTTP)
    // Asegurarse de que la respuesta HTTP es la esperada (ej. StatusCode::CREATED).
    assert_eq!(response.status(), StatusCode::CREATED);
    
    // 5. Verificación de Resultados (Persistencia y Efectos Secundarios)
    // Acceder directamente a las bases de datos o sistemas de almacenamiento para verificar que los datos se guardaron correctamente.
    let db = context.mongo_client.database("test_db");
    // ... verificaciones de base de datos, S3, o eventos publicados en RabbitMQ
}
```

## 5. Dependencias

Las dependencias clave del crate `artifact` son:

- **Runtime**: `axum` (framework web), `mongodb` (cliente MongoDB), `aws-sdk-s3` (cliente S3), `lapin` (cliente
  RabbitMQ), `bytes` (manejo de bytes), `serde` (serialización/deserialización).
- **Testing**: `reqwest` (cliente HTTP para tests), `testcontainers` (gestión de contenedores Docker para tests),
  `tokio-test` (utilidades de testing para Tokio).
- **Features**: `integration` (feature flag para habilitar los tests de integración).

Para versiones específicas, consulte el archivo `Cargo.toml` en la raíz de este crate.
