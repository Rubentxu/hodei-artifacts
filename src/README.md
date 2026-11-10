# Hodei Artifacts API - Composition Root

Este directorio contiene la aplicación principal de Hodei Artifacts API, implementando el **Composition Root** completo con Axum y todas las configuraciones de arranque.

## 📁 Estructura

```
src/
├── main.rs           # Punto de entrada principal
├── lib.rs            # Biblioteca con exports públicos
├── app_state.rs      # Estado de la aplicación con use cases
├── bootstrap.rs      # Lógica de inicialización y DI
├── config.rs         # Configuración desde environment
└── handlers/         # HTTP handlers de Axum
    ├── mod.rs        # Módulo de handlers
    ├── health.rs     # Health checks
    ├── policies.rs   # Validación y evaluación de políticas
    └── schemas.rs    # Gestión de schemas
```

## 🏗️ Arquitectura

### Composition Root Pattern

El archivo `bootstrap.rs` implementa el **Composition Root**, donde:

1. **Se instancian todos los adaptadores de infraestructura** (storage, DB, etc.)
2. **Se crea el `EngineBuilder` compartido** para construcción de schemas
3. **Se crean todos los use cases** con inyección de dependencias vía constructores
4. **Se registra el schema IAM** automáticamente en el arranque (configurable)
5. **Se construye el `AppState`** con todos los use cases como trait objects

### Flujo de Inicialización

```rust
main.rs
  ├── Config::from_env()                    // 1. Cargar configuración
  ├── initialize_logging()                  // 2. Inicializar logging
  ├── bootstrap()                           // 3. Composition Root
  │   ├── initialize_schema_storage()       //    a. Crear adaptadores
  │   ├── create_engine_builder()           //    b. Shared builder
  │   ├── create_use_cases()                //    c. Instanciar use cases
  │   │   ├── RegisterEntityTypeUseCase
  │   │   ├── RegisterActionTypeUseCase
  │   │   ├── BuildSchemaUseCase
  │   │   ├── LoadSchemaUseCase
  │   │   ├── ValidatePolicyUseCase
  │   │   ├── EvaluatePoliciesUseCase
  │   │   └── RegisterIamSchemaUseCase
  │   ├── register_iam_schema.execute()     //    d. Registrar IAM schema
  │   └── AppState::new()                   //    e. Crear estado
  ├── build_router()                        // 4. Configurar Axum
  └── axum::serve()                         // 5. Arrancar servidor
```

## 🎯 AppState

El `AppState` contiene todos los use cases como `Arc<UseCase>` para ser clonado en cada handler:

```rust
pub struct AppState<S: SchemaStoragePort> {
    pub schema_version: String,
    pub register_iam_schema: Arc<RegisterIamSchemaUseCase<S>>,
    pub register_entity_type: Arc<RegisterEntityTypeUseCase>,
    pub register_action_type: Arc<RegisterActionTypeUseCase>,
    pub build_schema: Arc<BuildSchemaUseCase<S>>,
    pub load_schema: Arc<LoadSchemaUseCase<S>>,
    pub validate_policy: Arc<ValidatePolicyUseCase<S>>,
    pub evaluate_policies: Arc<EvaluatePoliciesUseCase>,
}
```

## ⚙️ Configuración

La configuración se carga desde variables de entorno con valores por defecto:

### Variables de Entorno

#### Servidor
- `HODEI_SERVER_HOST` (default: `0.0.0.0`)
- `HODEI_SERVER_PORT` (default: `3000`)
- `HODEI_SERVER_REQUEST_TIMEOUT_SECS` (default: `30`)
- `HODEI_SERVER_MAX_BODY_SIZE` (default: `10485760` - 10MB)

#### Base de Datos
- `HODEI_DATABASE_TYPE` (default: `in-memory`)
- `HODEI_DATABASE_URL` (default: `memory://`)
- `HODEI_DATABASE_NAMESPACE` (opcional)
- `HODEI_DATABASE_NAME` (opcional)
- `HODEI_DATABASE_POOL_SIZE` (default: `10`)

#### Schema
- `HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP` (default: `true`)
- `HODEI_SCHEMA_VERSION` (opcional)
- `HODEI_SCHEMA_VALIDATE` (default: `true`)
- `HODEI_SCHEMA_STORAGE_TYPE` (default: `in-memory`)

#### Logging
- `HODEI_LOGGING_LEVEL` (default: `info`) - Valores: `trace`, `debug`, `info`, `warn`, `error`
- `HODEI_LOGGING_FORMAT` (default: `pretty`) - Valores: `pretty`, `json`, `compact`
- `HODEI_LOGGING_INCLUDE_TIMESTAMPS` (default: `true`)
- `HODEI_LOGGING_INCLUDE_LOCATION` (default: `false`)

### Ejemplo de Configuración

```bash
# Servidor
export HODEI_SERVER_HOST=127.0.0.1
export HODEI_SERVER_PORT=8080

# Schema
export HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP=true
export HODEI_SCHEMA_VERSION=v1.0.0

# Logging
export HODEI_LOGGING_LEVEL=debug
export HODEI_LOGGING_FORMAT=json
```

## 🚀 Arranque

### Desarrollo

```bash
# Con configuración por defecto
cargo run --bin hodei-artifacts-api

# Con configuración personalizada
HODEI_SERVER_PORT=8080 \
HODEI_LOGGING_LEVEL=debug \
cargo run --bin hodei-artifacts-api
```

### Producción

```bash
# Build optimizado
cargo build --release --bin hodei-artifacts-api

# Ejecutar
./target/release/hodei-artifacts-api
```

## 📡 Endpoints Disponibles

### Health Checks

- `GET /health` - Health check básico
- `GET /health/ready` - Readiness probe
- `GET /health/live` - Liveness probe

**Respuesta:**
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "service": "hodei-artifacts-api",
  "timestamp": "2024-01-15T10:30:00Z"
}
```

### Gestión de Schemas

#### Construir Schema
- `POST /api/v1/schemas/build`

**Request:**
```json
{
  "version": "v1.0.0",
  "validate": true
}
```

**Response:**
```json
{
  "entity_count": 2,
  "action_count": 6,
  "version": "v1.0.0",
  "validated": true,
  "schema_id": "schema_xyz"
}
```

#### Cargar Schema
- `GET /api/v1/schemas/load`

#### Registrar Schema IAM
- `POST /api/v1/schemas/register-iam`

**Request:**
```json
{
  "version": "v1.0.0",
  "validate": true
}
```

**Response:**
```json
{
  "entity_types_registered": 2,
  "action_types_registered": 6,
  "schema_version": "v1.0.0",
  "schema_id": "schema_abc",
  "validated": true
}
```

### Políticas

#### Validar Política
- `POST /api/v1/policies/validate`

**Request:**
```json
{
  "content": "permit(principal, action, resource);",
  "use_schema": true
}
```

**Response:**
```json
{
  "is_valid": true,
  "errors": [],
  "warnings": []
}
```

#### Evaluar Políticas
- `POST /api/v1/policies/evaluate`

**Request:**
```json
{
  "principal_hrn": "hrn:aws:iam::123:user/alice",
  "action": "CreateUser",
  "resource_hrn": "hrn:aws:iam::123:user/bob",
  "policies": [
    "permit(principal, action, resource);"
  ],
  "context": {},
  "evaluation_mode": "BestEffortNoSchema"
}
```

**Response:**
```json
{
  "decision": "Allow",
  "determining_policies": [],
  "reasons": [],
  "used_schema_version": "v1.0.0",
  "policy_ids_evaluated": ["policy_0"],
  "diagnostics": []
}
```

## 🧪 Testing

### Tests Unitarios

```bash
# Tests del composition root
cargo test --lib

# Tests específicos del bootstrap
cargo test --lib bootstrap
```

### Tests de Integración

```bash
# Tests de la aplicación completa
cargo test --test '*'
```

## 🔧 Extensibilidad

### Añadir un Nuevo Use Case

1. **Crear el use case** en el crate correspondiente (`hodei-iam`, `hodei-policies`, etc.)
2. **Añadirlo a `AppState`** en `src/app_state.rs`:
```rust
pub struct AppState<S> {
    // ... existing fields
    pub my_new_use_case: Arc<MyNewUseCase>,
}
```
3. **Instanciarlo en `bootstrap.rs`** en `create_use_cases()`:
```rust
let my_new_use_case = Arc::new(MyNewUseCase::new(dependencies));
```
4. **Crear un handler** en `src/handlers/`:
```rust
pub async fn my_handler<S>(
    State(state): State<AppState<S>>,
    Json(request): Json<MyRequest>,
) -> Result<Json<MyResponse>, ApiError> {
    let result = state.my_new_use_case.execute(request).await?;
    Ok(Json(result))
}
```
5. **Añadir la ruta** en `main.rs`:
```rust
.route("/api/v1/my-endpoint", post(handlers::my_handler))
```

### Añadir Middleware

En `build_router()` en `main.rs`:

```rust
.layer(my_custom_middleware())
.layer(TraceLayer::new_for_http())
```

## 📊 Observabilidad

### Logging

La aplicación usa `tracing` para logging estructurado:

```rust
use tracing::{info, warn, error, debug, trace};

info!("Usuario creado exitosamente");
warn!(user_id = %user.id, "Usuario sin permisos");
error!(error = ?err, "Falló la operación");
```

### Métricas

Los spans de `tracing` pueden exportarse a sistemas de métricas como Prometheus, Jaeger, etc.

## 🔒 Seguridad

### CORS

Configurado con allowlist explícita (`security.cors_allowed_origins`). Vacío por defecto: sin acceso cross-origin. `*` y orígenes no canónicos se rechazan al validar la configuración (la barra final se acepta y se normaliza).

```toml
[security]
cors_allowed_origins = ["https://app.hodei.io"]
```

La capa CORS no sustituye autenticación: con la superficie admin cerrada, un origen permitido sigue recibiendo `403`.

### Timeouts

Timeout configurable por request (default: 30s) vía `HODEI_SERVER_REQUEST_TIMEOUT_SECS`.

### Body Size Limits

Límite de tamaño de body configurable (default: 10MB) vía `HODEI_SERVER_MAX_BODY_SIZE`.

## 🐛 Troubleshooting

### El servidor no arranca

Verificar:
1. Puerto disponible: `lsof -i :3000`
2. Variables de entorno correctas
3. Logs de error en la salida

### Error "Failed to register IAM schema"

Verificar:
1. Storage de schemas accesible
2. `HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP=true`
3. Logs de tracing para más detalles

### Tests fallan

```bash
# Limpiar y reconstruir
cargo clean
cargo build --lib
cargo test --lib
```

## 📚 Referencias

- [Axum Documentation](https://docs.rs/axum/latest/axum/)
- [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)
- [Composition Root Pattern](https://blog.ploeh.dk/2011/07/28/CompositionRoot/)
- [Dependency Injection in Rust](https://www.lpalmieri.com/posts/dependency-injection-rust/)

## ✅ Estado Actual

- [x] Composition Root implementado
- [x] AppState con use cases
- [x] Configuración desde environment
- [x] Bootstrap con registro de IAM schema
- [x] Health check endpoints
- [x] Handlers de schemas (build, load, register-iam)
- [x] Handlers de policies (validate, evaluate - stub)
- [x] Logging estructurado con tracing
- [x] Graceful shutdown
- [x] Middleware (CORS, Timeout, Tracing)
- [ ] Autenticación/Autorización
- [ ] Rate limiting
- [ ] OpenAPI/Swagger documentation
- [ ] Implementación completa de evaluate_policies handler
- [ ] Adaptador SurrealDB para producción
- [ ] Métricas con Prometheus
- [ ] Distributed tracing con Jaeger