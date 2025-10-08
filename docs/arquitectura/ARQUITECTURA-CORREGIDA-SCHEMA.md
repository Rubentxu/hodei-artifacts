# Arquitectura Corregida: Schema Management con EngineBuilder

## 🎯 Corrección Fundamental

El usuario tiene razón: **NO necesitamos ports para el registro de tipos**. El `EngineBuilder` ya existe y funciona correctamente recibiendo tipos genéricos que implementan los traits del kernel.

## ✅ Arquitectura Correcta

### Principios Clave

1. **EngineBuilder es síncrono**: Registro en memoria, no requiere async
2. **Tipos genéricos directos**: `register_entity<T: HodeiEntityType>()`, `register_action_type<A: ActionTrait>()`
3. **Schema fragments automáticos**: Se extraen de los traits (ya implementado)
4. **Persistencia interna**: El `BuildSchemaUseCase` persiste automáticamente
5. **Recuperación para validación/evaluación**: Los use cases cargan el schema persistido

---

## 📦 EngineBuilder Existente (Ya Implementado)

**`crates/hodei-policies/src/internal/engine/builder.rs`**

```rust
pub struct EngineBuilder {
    entity_fragments: HashMap<String, SchemaFragment>,
    action_fragments: Vec<SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self { ... }
    
    // ✅ YA IMPLEMENTADO: Registra entity types genéricos
    pub fn register_entity<T: HodeiEntityType>(&mut self) 
        -> Result<&mut Self, Box<CedarSchemaError>> { ... }
    
    // ✅ YA IMPLEMENTADO: Registra action types genéricos
    pub fn register_action_type<A: ActionTrait>(&mut self) 
        -> Result<&mut Self, Box<CedarSchemaError>> { ... }
    
    // ✅ YA IMPLEMENTADO: Registra instancias de entidades
    pub fn register_entity_instance(&mut self, entity: &dyn HodeiEntity) 
        -> Result<&mut Self, Box<CedarSchemaError>> { ... }
    
    // ✅ YA IMPLEMENTADO: Construye schema final
    pub fn build_schema(self) -> Result<Schema, Box<SchemaError>> { ... }
}
```

**Características:**
- ✅ Métodos genéricos que aceptan tipos
- ✅ Extracción automática de schema fragments
- ✅ Síncrono (operaciones en memoria)
- ✅ No requiere async
- ✅ No requiere ports para el registro

---

## 🏗️ Features Necesarias

### En `hodei-policies`

#### 1. Feature: `build_and_persist_schema` (NUEVO)

**Responsabilidad:** Construir schema final desde EngineBuilder Y persistirlo automáticamente

**Estructura VSA:**
```
crates/hodei-policies/src/features/build_and_persist_schema/
├── mod.rs
├── use_case.rs
├── ports.rs          # SchemaStoragePort (async)
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**`ports.rs`:**
```rust
use async_trait::async_trait;
use super::dto::PersistedSchemaView;
use super::error::BuildAndPersistSchemaError;

/// Port for schema storage operations
#[async_trait]
pub trait SchemaStoragePort: Send + Sync {
    /// Save schema to persistent storage
    async fn save_schema(
        &self,
        schema_content: String,
        schema_hash: String,
    ) -> Result<PersistedSchemaView, BuildAndPersistSchemaError>;

    /// Load latest schema from storage
    async fn load_latest_schema(
        &self,
    ) -> Result<Option<PersistedSchemaView>, BuildAndPersistSchemaError>;
}
```

**`dto.rs`:**
```rust
use chrono::{DateTime, Utc};

/// Command to build and persist schema
pub struct BuildAndPersistSchemaCommand {
    // Sin parámetros - usa el builder ya poblado
}

/// View of the final schema (built and persisted)
pub struct SchemaView {
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub entity_count: usize,
    pub action_count: usize,
    pub created_at: DateTime<Utc>,
    pub was_persisted: bool,  // true si se guardó, false si se reutilizó existente
}

/// View of persisted schema (from storage)
#[derive(Debug, Clone)]
pub struct PersistedSchemaView {
    pub id: String,
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub created_at: DateTime<Utc>,
}
```

**`use_case.rs`:**
```rust
use super::dto::{BuildAndPersistSchemaCommand, SchemaView, PersistedSchemaView};
use super::error::BuildAndPersistSchemaError;
use super::ports::SchemaStoragePort;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::Arc;
use tracing::{info, instrument, warn};
use sha2::{Sha256, Digest};
use chrono::Utc;

/// Use case for building and persisting Cedar schemas
///
/// This use case:
/// 1. Takes an already-populated EngineBuilder
/// 2. Calls build_schema() to generate the Cedar schema
/// 3. Compares with persisted schema
/// 4. Persists only if changed
pub struct BuildAndPersistSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    schema_storage: Arc<SS>,
}

impl<SS> BuildAndPersistSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    pub fn new(schema_storage: Arc<SS>) -> Self {
        Self { schema_storage }
    }

    #[instrument(skip(self, builder))]
    pub async fn execute(
        &self,
        builder: EngineBuilder,  // ← Toma ownership para consumir en build_schema()
        _command: BuildAndPersistSchemaCommand,
    ) -> Result<SchemaView, BuildAndPersistSchemaError> {
        info!("Building Cedar schema");

        // 1. Obtener counts antes de consumir el builder
        let entity_count = builder.entity_count();
        let action_count = builder.action_count();

        // 2. Construir schema (consume el builder)
        let schema = builder.build_schema()
            .map_err(|e| BuildAndPersistSchemaError::SchemaBuilderError(e.to_string()))?;

        // 3. Convertir a JSON para persistencia
        let schema_json = serde_json::to_string_pretty(&schema)
            .map_err(|e| BuildAndPersistSchemaError::SerializationError(e.to_string()))?;

        // 4. Calcular hash
        let mut hasher = Sha256::new();
        hasher.update(&schema_json);
        let schema_hash = format!("{:x}", hasher.finalize());

        info!(
            entity_count = entity_count,
            action_count = action_count,
            schema_hash = %schema_hash,
            "Schema built successfully"
        );

        // 5. Comparar con schema existente
        let existing = self.schema_storage.load_latest_schema().await?;
        let was_persisted = if let Some(existing_schema) = existing {
            if existing_schema.schema_hash == schema_hash {
                info!("Schema unchanged, reusing existing");
                return Ok(SchemaView {
                    schema_content: existing_schema.schema_content,
                    schema_hash: existing_schema.schema_hash,
                    version: existing_schema.version,
                    entity_count,
                    action_count,
                    created_at: existing_schema.created_at,
                    was_persisted: false,
                });
            }
            warn!("Schema changed, persisting new version");
            true
        } else {
            info!("No existing schema, persisting for first time");
            true
        };

        // 6. Persistir nuevo schema
        if was_persisted {
            let persisted = self
                .schema_storage
                .save_schema(schema_json.clone(), schema_hash.clone())
                .await?;

            info!(version = %persisted.version, "Schema persisted successfully");

            Ok(SchemaView {
                schema_content: persisted.schema_content,
                schema_hash: persisted.schema_hash,
                version: persisted.version,
                entity_count,
                action_count,
                created_at: persisted.created_at,
                was_persisted: true,
            })
        } else {
            unreachable!("This branch should not be reached")
        }
    }
}
```

---

#### 2. Feature: `load_schema` (NUEVO)

**Responsabilidad:** Cargar schema persistido para uso en validación/evaluación

**Estructura VSA:**
```
crates/hodei-policies/src/features/load_schema/
├── mod.rs
├── use_case.rs
├── ports.rs          # SchemaLoaderPort (async)
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**`ports.rs`:**
```rust
use async_trait::async_trait;
use cedar_policy::Schema;
use super::error::LoadSchemaError;

/// Port for loading schemas
#[async_trait]
pub trait SchemaLoaderPort: Send + Sync {
    /// Load the latest schema as Cedar Schema object
    async fn load_schema(&self) -> Result<Schema, LoadSchemaError>;
}
```

**`dto.rs`:**
```rust
/// Command to load schema (no parameters)
pub struct LoadSchemaCommand;

/// View of loaded schema
pub struct LoadedSchemaView {
    pub schema: cedar_policy::Schema,
    pub version: String,
}
```

**`use_case.rs`:**
```rust
use super::dto::{LoadSchemaCommand, LoadedSchemaView};
use super::error::LoadSchemaError;
use super::ports::SchemaLoaderPort;
use std::sync::Arc;
use tracing::{info, instrument};

/// Use case for loading persisted schemas
pub struct LoadSchemaUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    schema_loader: Arc<SL>,
}

impl<SL> LoadSchemaUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    pub fn new(schema_loader: Arc<SL>) -> Self {
        Self { schema_loader }
    }

    #[instrument(skip(self, _command))]
    pub async fn execute(
        &self,
        _command: LoadSchemaCommand,
    ) -> Result<LoadedSchemaView, LoadSchemaError> {
        info!("Loading persisted Cedar schema");

        let schema = self.schema_loader.load_schema().await?;

        info!("Schema loaded successfully");

        Ok(LoadedSchemaView {
            schema,
            version: "loaded".to_string(), // TODO: obtener version real
        })
    }
}
```

---

#### 3. Modificar: `validate_policy` (EXISTENTE)

**Cambio:** Agregar dependencia en `SchemaLoaderPort` para validar con schema

**`ports.rs`** (agregar):
```rust
pub use crate::features::load_schema::SchemaLoaderPort;
```

**`use_case.rs`** (modificar):
```rust
pub struct ValidatePolicyUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    schema_loader: Arc<SL>,  // ← NUEVO
}

impl<SL> ValidatePolicyUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    pub fn new(schema_loader: Arc<SL>) -> Self {
        Self { schema_loader }
    }

    pub async fn execute(&self, command: ValidatePolicyCommand) 
        -> Result<ValidationResult, ValidatePolicyError> 
    {
        // 1. Cargar schema
        let schema = self.schema_loader.load_schema().await?;
        
        // 2. Validar política contra schema
        // ... usar Cedar validator con schema ...
    }
}
```

---

#### 4. Modificar: `evaluate_policies` (EXISTENTE)

**Cambio:** Agregar dependencia en `SchemaLoaderPort` para evaluar con schema

**Similar a validate_policy**

---

### En `hodei-iam`

#### Feature: `register_iam_types` (NUEVO)

**Responsabilidad:** Registrar todos los entity types y action types de IAM

**Estructura VSA:**
```
crates/hodei-iam/src/features/register_iam_types/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**NO tiene `ports.rs`** porque usa directamente `EngineBuilder`

**`use_case.rs`:**
```rust
use crate::internal::domain::{User, Group, Account};
use crate::internal::actions::*;
use super::dto::{RegisterIamTypesCommand, IamTypesView};
use super::error::RegisterIamTypesError;
use hodei_policies::internal::engine::builder::EngineBuilder;
use tracing::{info, instrument};

/// Use case for registering IAM types in the schema builder
///
/// This use case registers all IAM entity types and action types
/// directly on the provided EngineBuilder.
pub struct RegisterIamTypesUseCase;

impl RegisterIamTypesUseCase {
    pub fn new() -> Self {
        Self
    }

    #[instrument(skip(self, builder))]
    pub fn execute(
        &self,
        builder: &mut EngineBuilder,  // ← Recibe &mut EngineBuilder directamente
        _command: RegisterIamTypesCommand,
    ) -> Result<IamTypesView, RegisterIamTypesError> {
        info!("Registering IAM types");

        let mut entity_types = Vec::new();
        let mut action_types = Vec::new();

        // Registrar entity types
        builder.register_entity::<User>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        entity_types.push("Iam::User".to_string());

        builder.register_entity::<Group>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        entity_types.push("Iam::Group".to_string());

        builder.register_entity::<Account>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        entity_types.push("Iam::Account".to_string());

        // Registrar action types
        builder.register_action_type::<CreateUserAction>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        action_types.push("CreateUser".to_string());

        builder.register_action_type::<UpdateUserAction>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        action_types.push("UpdateUser".to_string());

        builder.register_action_type::<DeleteUserAction>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        action_types.push("DeleteUser".to_string());

        builder.register_action_type::<CreateGroupAction>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        action_types.push("CreateGroup".to_string());

        builder.register_action_type::<AddToGroupAction>()
            .map_err(|e| RegisterIamTypesError::RegistrationError(e.to_string()))?;
        action_types.push("AddToGroup".to_string());

        // ... más actions ...

        info!(
            entity_count = entity_types.len(),
            action_count = action_types.len(),
            "IAM types registered successfully"
        );

        Ok(IamTypesView {
            entity_types_registered: entity_types,
            action_types_registered: action_types,
        })
    }
}
```

**`dto.rs`:**
```rust
/// Command to register IAM types (no parameters)
pub struct RegisterIamTypesCommand;

/// View of registered IAM types
pub struct IamTypesView {
    pub entity_types_registered: Vec<String>,
    pub action_types_registered: Vec<String>,
}
```

---

#### Módulo: `internal/actions/` (NUEVO)

```
crates/hodei-iam/src/internal/actions/
├── mod.rs
├── create_user_action.rs
├── update_user_action.rs
├── delete_user_action.rs
├── create_group_action.rs
├── update_group_action.rs
├── delete_group_action.rs
├── add_to_group_action.rs
└── remove_from_group_action.rs
```

**Ejemplo: `create_user_action.rs`:**
```rust
use kernel::{ActionTrait, ServiceName};

/// Action for creating a new user
pub struct CreateUserAction;

impl ActionTrait for CreateUserAction {
    fn name() -> &'static str {
        "CreateUser"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").unwrap()
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Account".to_string()
    }
}
```

---

## 🚀 Flujo de Arranque Completo

### En `main.rs` (Composition Root)

```rust
use std::sync::Arc;
use hodei_policies::internal::engine::builder::EngineBuilder;
use hodei_policies::features::{build_and_persist_schema, load_schema};
use hodei_iam::features::register_iam_types;
// ... otros bounded contexts

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Inicializar infraestructura
    let db = initialize_surrealdb().await?;
    
    // 2. Crear EngineBuilder
    let mut builder = EngineBuilder::new();
    
    // 3. Registrar tipos de todos los bounded contexts
    info!("Registering schema types from all bounded contexts");
    
    // 3.1. Registrar tipos IAM
    let register_iam = register_iam_types::RegisterIamTypesUseCase::new();
    register_iam.execute(
        &mut builder,
        register_iam_types::dto::RegisterIamTypesCommand,
    )?;
    
    // 3.2. Registrar tipos Organizations
    let register_orgs = hodei_organizations::features::register_org_types::RegisterOrgTypesUseCase::new();
    register_orgs.execute(
        &mut builder,
        hodei_organizations::features::register_org_types::dto::RegisterOrgTypesCommand,
    )?;
    
    // 3.3. Registrar tipos Artifacts
    let register_artifacts = hodei_artifacts::features::register_artifact_types::RegisterArtifactTypesUseCase::new();
    register_artifacts.execute(
        &mut builder,
        hodei_artifacts::features::register_artifact_types::dto::RegisterArtifactTypesCommand,
    )?;
    
    info!("All types registered. Building and persisting schema...");
    
    // 4. Construir y persistir schema
    let schema_storage = Arc::new(
        hodei_policies::infrastructure::surreal::SurrealSchemaStorageAdapter::new(db.clone())
    );
    
    let build_and_persist = build_and_persist_schema::BuildAndPersistSchemaUseCase::new(schema_storage.clone());
    let schema_view = build_and_persist.execute(
        builder,  // ← Pasa ownership del builder (se consume en build_schema)
        build_and_persist_schema::dto::BuildAndPersistSchemaCommand,
    ).await?;
    
    info!(
        version = %schema_view.version,
        entity_count = schema_view.entity_count,
        action_count = schema_view.action_count,
        was_persisted = schema_view.was_persisted,
        "Schema ready"
    );
    
    // 5. Inicializar use cases que necesitan schema
    let schema_loader = Arc::new(
        hodei_policies::infrastructure::surreal::SurrealSchemaLoaderAdapter::new(db.clone())
    );
    
    let validate_policy_uc = hodei_policies::features::validate_policy::ValidatePolicyUseCase::new(
        schema_loader.clone()
    );
    
    let evaluate_policies_uc = hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase::new(
        schema_loader.clone()
    );
    
    // 6. Inicializar app state
    let app_state = AppState {
        validate_policy: Arc::new(validate_policy_uc),
        evaluate_policies: Arc::new(evaluate_policies_uc),
        db,
    };
    
    // 7. Arrancar servidor
    start_server(app_state).await?;
    
    Ok(())
}
```

---

## 📊 Diferencias con la Arquitectura Anterior

### ❌ Arquitectura Anterior (INCORRECTA)

```rust
// Tenía ports innecesarios para registro
pub trait SchemaBuilderPort {
    async fn register_entity_type(&self, name: String, fragment: String) -> ...;
}

// Features separadas para cada operación de registro
- register_entity_type (feature completa) ❌
- register_action_type (feature completa) ❌
```

**Problemas:**
- Port innecesario (registro es síncrono en memoria)
- Pasaba strings en lugar de tipos
- Features demasiado granulares

### ✅ Arquitectura Corregida (CORRECTA)

```rust
// Usa EngineBuilder directamente (ya existe)
impl EngineBuilder {
    pub fn register_entity<T: HodeiEntityType>(&mut self) -> ...;
    pub fn register_action_type<A: ActionTrait>(&mut self) -> ...;
}

// Features agrupadas por bounded context
- register_iam_types (registra todos los tipos IAM) ✅
- register_org_types (registra todos los tipos Org) ✅
```

**Ventajas:**
- Usa EngineBuilder directamente (sin port intermedio)
- Tipos genéricos (type safety)
- Schema fragments automáticos
- Más simple y directo

---

## 🎯 Resumen de Features Necesarias

### hodei-policies (3 features)

1. ✅ **`build_and_persist_schema`** - Construir y persistir automáticamente
2. ✅ **`load_schema`** - Cargar schema persistido
3. ✅ **Modificar `validate_policy`** - Usar schema cargado
4. ✅ **Modificar `evaluate_policies`** - Usar schema cargado

### hodei-iam (1 feature)

1. ✅ **`register_iam_types`** - Registrar todos los tipos IAM

### Otros bounded contexts (1 feature cada uno)

1. ✅ **`register_org_types`** en hodei-organizations
2. ✅ **`register_artifact_types`** en hodei-artifacts

---

## ⏱️ Tiempo Estimado Corregido

| Tarea | Tiempo |
|-------|--------|
| build_and_persist_schema (feature) | 2 horas |
| load_schema (feature) | 1.5 horas |
| Modificar validate_policy | 1 hora |
| Modificar evaluate_policies | 1 hora |
| Adaptador SurrealDB | 1.5 horas |
| register_iam_types (feature) | 1.5 horas |
| Actions IAM (módulo) | 2 horas |
| Integración main.rs | 2 horas |
| **TOTAL** | **~12.5 horas** |

---

## ✅ Criterios de Aceptación

- [ ] EngineBuilder se usa directamente (sin port intermedio)
- [ ] Registro de tipos es síncrono (no async)
- [ ] Tipos genéricos (`T: HodeiEntityType`, `A: ActionTrait`)
- [ ] Schema se construye y persiste automáticamente
- [ ] Schema se carga para validación/evaluación
- [ ] Solo se persiste si el schema cambió
- [ ] Todos los tests pasan
- [ ] Código compila sin warnings

---

## 🚀 Próximo Paso Inmediato

**Empezar con:**
1. Crear `EvaluatePoliciesPort` en hodei-policies (Fase 1.1)
2. Actualizar hodei-iam para usar el port (Fase 1.2)
3. Implementar `build_and_persist_schema` feature
4. Implementar `load_schema` feature
5. Modificar `validate_policy` y `evaluate_policies`
6. Implementar `register_iam_types` feature
7. Integrar en main.rs

---

## 📚 Conclusión

La arquitectura corregida:
- ✅ **Más simple**: Usa EngineBuilder directamente
- ✅ **Type-safe**: Tipos genéricos en lugar de strings
- ✅ **Eficiente**: Registro síncrono, persistencia async solo cuando cambia
- ✅ **Flexible**: Cada BC registra sus tipos independientemente
- ✅ **Mantenible**: Features agrupadas lógicamente por BC

El schema se convierte en un artefacto de primera clase que se construye dinámicamente al arranque y se reutiliza eficientemente en reinicios.