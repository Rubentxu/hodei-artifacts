# Arquitectura SIMPLE y FINAL: Schema Management Síncrono

## 🎯 Principio Fundamental

**SIMPLE ES MEJOR**

- ✅ Use cases **SÍNCRONOS** para registro (operaciones en memoria)
- ✅ Métodos **genéricos directos** (`register<T: HodeiEntityType>()`)
- ✅ **SIN comandos**, **SIN closures**, **SIN indirecciones**
- ✅ EngineBuilder es **INTERNO** a hodei-policies, **NUNCA se expone**

---

## 🏗️ Arquitectura General

```
┌─────────────────────────────────────────────────────────────┐
│                    main.rs (Composition Root)               │
└────────────────────────────┬────────────────────────────────┘
                             │
                             │ 1. Crear use cases síncronos
                             ▼
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  hodei-iam   │    │hodei-orgs    │    │hodei-artifacts│
│              │    │              │    │              │
│register_iam_ │    │register_org_ │    │register_art_ │
│schema UC     │    │schema UC     │    │schema UC     │
│(SÍNCRONO)    │    │(SÍNCRONO)    │    │(SÍNCRONO)    │
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                    │
       │ .register<User>()                      │
       │ .register<CreateUserAction>()          │
       └───────────────────┼────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              hodei-policies (SÍNCRONO)                      │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  RegisterEntityTypeUseCase (SÍNCRONO)               │   │
│  │  pub fn register<T: HodeiEntityType>(&self)         │   │
│  │      → builder.lock().register_entity::<T>()        │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  RegisterActionTypeUseCase (SÍNCRONO)               │   │
│  │  pub fn register<A: ActionTrait>(&self)             │   │
│  │      → builder.lock().register_action_type::<A>()   │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  BuildSchemaUseCase (ASYNC)                         │   │
│  │  pub async fn execute(&self)                        │   │
│  │      → builder.build_schema()                       │   │
│  │      → storage.save() si cambió                     │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌──────────────────────────────────────┐                  │
│  │  EngineBuilder (INTERNO)             │                  │
│  │  - Arc<Mutex<EngineBuilder>>         │                  │
│  │  - NUNCA se expone                   │                  │
│  └──────────────────────────────────────┘                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 📦 hodei-policies: Features

### Feature 1: `register_entity_type` (SÍNCRONO)

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_entity_type/
├── mod.rs
├── use_case.rs        # SÍNCRONO
├── error.rs
├── di.rs
└── use_case_test.rs
```

**NO tiene:**
- ❌ `dto.rs` - No hay comandos
- ❌ `ports.rs` - No depende de externos

#### `use_case.rs`

```rust
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};
use tracing::{info, instrument};
use super::error::RegisterEntityTypeError;

/// Use case for registering entity types in the schema builder
///
/// This use case is SYNCHRONOUS because it only operates on in-memory data.
pub struct RegisterEntityTypeUseCase {
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterEntityTypeUseCase {
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    /// Register an entity type directly using the generic type
    ///
    /// # Example
    ///
    /// ```rust
    /// use hodei_iam::internal::domain::User;
    /// 
    /// use_case.register::<User>()?;
    /// ```
    #[instrument(skip(self))]
    pub fn register<T: kernel::HodeiEntityType + 'static>(
        &self,
    ) -> Result<(), RegisterEntityTypeError> {
        info!(
            entity_type = format!("{}::{}", T::service_name(), T::resource_type_name()),
            "Registering entity type"
        );

        let mut builder = self.builder.lock()
            .map_err(|_| RegisterEntityTypeError::BuilderLocked)?;

        builder.register_entity::<T>()
            .map_err(|e| RegisterEntityTypeError::RegistrationFailed(e.to_string()))?;

        info!("Entity type registered successfully");
        Ok(())
    }
}
```

#### `error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterEntityTypeError {
    #[error("Builder locked")]
    BuilderLocked,
    
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),
}
```

#### `di.rs`

```rust
use super::use_case::RegisterEntityTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

pub struct RegisterEntityTypeUseCaseFactory;

impl RegisterEntityTypeUseCaseFactory {
    pub fn build(builder: Arc<Mutex<EngineBuilder>>) -> RegisterEntityTypeUseCase {
        RegisterEntityTypeUseCase::new(builder)
    }
}
```

---

### Feature 2: `register_action_type` (SÍNCRONO)

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_action_type/
├── mod.rs
├── use_case.rs        # SÍNCRONO
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `use_case.rs`

```rust
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};
use tracing::{info, instrument};
use super::error::RegisterActionTypeError;

/// Use case for registering action types in the schema builder
///
/// This use case is SYNCHRONOUS because it only operates on in-memory data.
pub struct RegisterActionTypeUseCase {
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterActionTypeUseCase {
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    /// Register an action type directly using the generic type
    ///
    /// # Example
    ///
    /// ```rust
    /// use hodei_iam::internal::actions::CreateUserAction;
    /// 
    /// use_case.register::<CreateUserAction>()?;
    /// ```
    #[instrument(skip(self))]
    pub fn register<A: kernel::ActionTrait + 'static>(
        &self,
    ) -> Result<(), RegisterActionTypeError> {
        info!(action_name = A::name(), "Registering action type");

        let mut builder = self.builder.lock()
            .map_err(|_| RegisterActionTypeError::BuilderLocked)?;

        builder.register_action_type::<A>()
            .map_err(|e| RegisterActionTypeError::RegistrationFailed(e.to_string()))?;

        info!("Action type registered successfully");
        Ok(())
    }
}
```

---

### Feature 3: `build_schema` (ASYNC)

**Estructura VSA:**
```
crates/hodei-policies/src/features/build_schema/
├── mod.rs
├── use_case.rs        # ASYNC (accede a DB)
├── ports.rs           # SchemaStoragePort
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `ports.rs`

```rust
use async_trait::async_trait;
use super::dto::PersistedSchemaView;
use super::error::BuildSchemaError;

#[async_trait]
pub trait SchemaStoragePort: Send + Sync {
    async fn save_schema(
        &self,
        schema_content: String,
        schema_hash: String,
    ) -> Result<PersistedSchemaView, BuildSchemaError>;

    async fn load_latest_schema(
        &self,
    ) -> Result<Option<PersistedSchemaView>, BuildSchemaError>;
}
```

#### `dto.rs`

```rust
use chrono::{DateTime, Utc};

/// View of the final schema
pub struct SchemaView {
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub entity_count: usize,
    pub action_count: usize,
    pub created_at: DateTime<Utc>,
    pub was_persisted: bool,
}

#[derive(Debug, Clone)]
pub struct PersistedSchemaView {
    pub id: String,
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub created_at: DateTime<Utc>,
}
```

#### `use_case.rs`

```rust
use super::dto::{SchemaView, PersistedSchemaView};
use super::error::BuildSchemaError;
use super::ports::SchemaStoragePort;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};
use tracing::{info, instrument, warn};
use sha2::{Sha256, Digest};
use chrono::Utc;

/// Use case for building and persisting the final Cedar schema
///
/// This use case is ASYNC because it persists to SurrealDB.
pub struct BuildSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    builder: Arc<Mutex<EngineBuilder>>,
    storage: Arc<SS>,
}

impl<SS> BuildSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    pub fn new(builder: Arc<Mutex<EngineBuilder>>, storage: Arc<SS>) -> Self {
        Self { builder, storage }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self) -> Result<SchemaView, BuildSchemaError> {
        info!("Building final Cedar schema");

        // Get counts before consuming builder
        let entity_count;
        let action_count;
        {
            let builder_guard = self.builder.lock()
                .map_err(|_| BuildSchemaError::BuilderLocked)?;
            entity_count = builder_guard.entity_count();
            action_count = builder_guard.action_count();
        }

        // Take ownership of builder to build schema
        let builder = Arc::try_unwrap(self.builder.clone())
            .map_err(|_| BuildSchemaError::BuilderInUse)?
            .into_inner()
            .map_err(|_| BuildSchemaError::BuilderLocked)?;

        let schema = builder.build_schema()
            .map_err(|e| BuildSchemaError::SchemaGenerationError(e.to_string()))?;

        // Serialize
        let schema_json = serde_json::to_string_pretty(&schema)
            .map_err(|e| BuildSchemaError::SerializationError(e.to_string()))?;

        // Calculate hash
        let mut hasher = Sha256::new();
        hasher.update(&schema_json);
        let schema_hash = format!("{:x}", hasher.finalize());

        info!(
            entity_count = entity_count,
            action_count = action_count,
            schema_hash = %schema_hash,
            "Schema built successfully"
        );

        // Compare with existing
        let existing = self.storage.load_latest_schema().await?;
        
        if let Some(existing_schema) = existing {
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
        }

        // Persist (INTERNAL)
        let persisted = self.storage.save_schema(schema_json, schema_hash).await?;

        info!(version = %persisted.version, "Schema persisted");

        Ok(SchemaView {
            schema_content: persisted.schema_content,
            schema_hash: persisted.schema_hash,
            version: persisted.version,
            entity_count,
            action_count,
            created_at: persisted.created_at,
            was_persisted: true,
        })
    }
}
```

---

## 📦 hodei-iam: Feature para Registro

### Feature: `register_iam_schema` (SÍNCRONO)

**Estructura VSA:**
```
crates/hodei-iam/src/features/register_iam_schema/
├── mod.rs
├── use_case.rs        # SÍNCRONO
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**NO tiene `ports.rs`** - Usa directamente los use cases de hodei-policies

#### `use_case.rs`

```rust
use super::dto::{RegisterIamSchemaCommand, IamSchemaView};
use super::error::RegisterIamSchemaError;
use crate::internal::domain::{User, Group, Account};
use crate::internal::actions::*;
use std::sync::Arc;
use tracing::{info, instrument};

use hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase;
use hodei_policies::features::register_action_type::RegisterActionTypeUseCase;

/// Use case for registering all IAM types at application startup
///
/// This use case is SYNCHRONOUS - it only registers types in memory.
pub struct RegisterIamSchemaUseCase {
    entity_registrar: Arc<RegisterEntityTypeUseCase>,
    action_registrar: Arc<RegisterActionTypeUseCase>,
}

impl RegisterIamSchemaUseCase {
    pub fn new(
        entity_registrar: Arc<RegisterEntityTypeUseCase>,
        action_registrar: Arc<RegisterActionTypeUseCase>,
    ) -> Self {
        Self {
            entity_registrar,
            action_registrar,
        }
    }

    #[instrument(skip(self))]
    pub fn execute(&self) -> Result<IamSchemaView, RegisterIamSchemaError> {
        info!("Registering IAM schema types");

        let mut entity_types = Vec::new();
        let mut action_types = Vec::new();

        // Register entity types - DIRECT calls with generic types
        self.entity_registrar.register::<User>()?;
        entity_types.push("Iam::User".to_string());

        self.entity_registrar.register::<Group>()?;
        entity_types.push("Iam::Group".to_string());

        self.entity_registrar.register::<Account>()?;
        entity_types.push("Iam::Account".to_string());

        // Register action types - DIRECT calls with generic types
        self.action_registrar.register::<CreateUserAction>()?;
        action_types.push("CreateUser".to_string());

        self.action_registrar.register::<UpdateUserAction>()?;
        action_types.push("UpdateUser".to_string());

        self.action_registrar.register::<DeleteUserAction>()?;
        action_types.push("DeleteUser".to_string());

        self.action_registrar.register::<CreateGroupAction>()?;
        action_types.push("CreateGroup".to_string());

        self.action_registrar.register::<AddToGroupAction>()?;
        action_types.push("AddToGroup".to_string());

        self.action_registrar.register::<RemoveFromGroupAction>()?;
        action_types.push("RemoveFromGroup".to_string());

        info!(
            entity_count = entity_types.len(),
            action_count = action_types.len(),
            "IAM schema types registered successfully"
        );

        Ok(IamSchemaView {
            entity_types_registered: entity_types,
            action_types_registered: action_types,
        })
    }
}
```

#### `dto.rs`

```rust
/// Command to register IAM schema (no parameters needed)
pub struct RegisterIamSchemaCommand;

/// View of registered IAM schema
pub struct IamSchemaView {
    pub entity_types_registered: Vec<String>,
    pub action_types_registered: Vec<String>,
}
```

#### `error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterIamSchemaError {
    #[error("Entity registration failed: {0}")]
    EntityRegistrationFailed(#[from] hodei_policies::features::register_entity_type::error::RegisterEntityTypeError),
    
    #[error("Action registration failed: {0}")]
    ActionRegistrationFailed(#[from] hodei_policies::features::register_action_type::error::RegisterActionTypeError),
}
```

---

## 🚀 Flujo de Arranque en main.rs

```rust
use std::sync::{Arc, Mutex};
use hodei_policies::internal::engine::builder::EngineBuilder;
use hodei_policies::features::{
    register_entity_type::RegisterEntityTypeUseCase,
    register_action_type::RegisterActionTypeUseCase,
    build_schema::BuildSchemaUseCase,
};
use hodei_iam::features::register_iam_schema::RegisterIamSchemaUseCase;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Inicializar infraestructura
    let db = initialize_surrealdb().await?;
    
    // 2. Crear EngineBuilder compartido (INTERNO)
    let builder = Arc::new(Mutex::new(EngineBuilder::new()));
    
    // 3. Crear use cases SÍNCRONOS de hodei-policies
    let register_entity_uc = Arc::new(RegisterEntityTypeUseCase::new(builder.clone()));
    let register_action_uc = Arc::new(RegisterActionTypeUseCase::new(builder.clone()));
    
    info!("Registering schema types from all bounded contexts");
    
    // 4. Registrar tipos de IAM (SÍNCRONO)
    let register_iam_uc = RegisterIamSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_iam_uc.execute()?;  // ← Sin .await porque es síncrono
    
    // 5. Registrar tipos de Organizations (SÍNCRONO)
    let register_orgs_uc = hodei_organizations::features::register_org_schema::RegisterOrgSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_orgs_uc.execute()?;  // ← Sin .await
    
    // 6. Registrar tipos de Artifacts (SÍNCRONO)
    let register_artifacts_uc = hodei_artifacts::features::register_artifact_schema::RegisterArtifactSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_artifacts_uc.execute()?;  // ← Sin .await
    
    info!("All types registered. Building schema...");
    
    // 7. Construir y persistir schema (ASYNC - accede a DB)
    let schema_storage = Arc::new(
        hodei_policies::infrastructure::surreal::SurrealSchemaStorageAdapter::new(db.clone())
    );
    
    let build_schema_uc = BuildSchemaUseCase::new(builder, schema_storage);
    let schema_view = build_schema_uc.execute().await?;  // ← Sí .await porque persiste
    
    info!(
        version = %schema_view.version,
        entity_count = schema_view.entity_count,
        action_count = schema_view.action_count,
        was_persisted = schema_view.was_persisted,
        "Schema ready"
    );
    
    // 8. Inicializar app state y arrancar servidor
    let app_state = initialize_app_state(db, schema_view).await?;
    start_server(app_state).await?;
    
    Ok(())
}
```

---

## ✅ Ventajas de Esta Arquitectura

### 1. SIMPLE y DIRECTA

```rust
// ✅ Llamada directa con el tipo
use_case.register::<User>()?;

// ❌ NO necesitamos esto
let command = RegisterEntityTypeCommand::new::<User>();
use_case.execute(command).await?;
```

### 2. SÍNCRONO donde Corresponde

- ✅ Registro: SÍNCRONO (memoria)
- ✅ Build + persist: ASYNC (DB)

### 3. Sin Capas Innecesarias

- ❌ NO comandos
- ❌ NO closures
- ❌ NO EntityTypeInfo/ActionTypeInfo
- ✅ Tipos directos

### 4. Type-Safe

```rust
// El compilador verifica que User implementa HodeiEntityType
use_case.register::<User>()?;
```

### 5. Respeta VSA

- ✅ Features completas
- ✅ Use cases
- ✅ EngineBuilder interno
- ✅ Bounded contexts desacoplados

---

## 📊 Comparación

### ❌ Complejidad Innecesaria

```rust
// Crear comando con closure
let command = RegisterEntityTypeCommand::new::<User>();

// Ejecutar async innecesario
use_case.execute(command).await?;

// El use case ejecuta el closure
(command.registration)(&mut builder)?;
```

### ✅ Simplicidad

```rust
// Llamada directa síncrona
use_case.register::<User>()?;
```

---

## ⏱️ Tiempo Estimado

| Tarea | Tiempo |
|-------|--------|
| Feature register_entity_type (síncrono) | 1h |
| Feature register_action_type (síncrono) | 1h |
| Feature build_schema (async) | 2h |
| Feature load_schema (async) | 1.5h |
| Modificar validate_policy | 1h |
| Modificar evaluate_policies | 1h |
| Adaptador SurrealDB | 1.5h |
| Feature register_iam_schema (síncrono) | 0.5h |
| Módulo actions IAM | 2h |
| Integración main.rs | 1.5h |
| **TOTAL** | **~13 horas** |

---

## 🎯 Criterios de Aceptación

- [ ] EngineBuilder NUNCA se expone fuera de hodei-policies
- [ ] Use cases de registro son SÍNCRONOS
- [ ] Métodos genéricos directos (`register<T>()`)
- [ ] SIN comandos, SIN closures, SIN indirecciones
- [ ] Solo `build_schema` es async (persiste en DB)
- [ ] Todo via features VSA con use cases
- [ ] Builder compartido via Arc<Mutex<>>
- [ ] Todos los tests pasan
- [ ] Código compila sin warnings

---

## 🚀 Estado

✅ **ARQUITECTURA SIMPLE y FINAL - LISTA PARA IMPLEMENTACIÓN**

**KISS: Keep It Simple, Stupid**

Esta es la solución más simple, directa y correcta posible.