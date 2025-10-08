# Arquitectura DEFINITIVA: Schema Management con Closures

## 🎯 Principio Fundamental

**El `EngineBuilder` es COMPLETAMENTE INTERNO a `hodei-policies`**

- ❌ NUNCA se expone directamente
- ✅ TODO se hace a través de **features VSA** con **use cases**
- ✅ Los comandos usan **closures** que capturan tipos en compile-time
- ✅ **SIN capa de indirección innecesaria** (EntityTypeInfo/ActionTypeInfo)

---

## 🏗️ Solución: Comandos con Closures

### Problema Original

Los traits async no pueden tener métodos genéricos:

```rust
#[async_trait]
pub trait RegisterEntityTypePort {
    async fn register<T: HodeiEntityType>(&self) -> Result<...>;
    // ❌ NO COMPILA - async traits no permiten genéricos
}
```

### Solución: Closures que Capturan Tipos

```rust
pub struct RegisterEntityTypeCommand {
    pub type_id: TypeId,
    pub registration: Box<dyn FnOnce(&mut EngineBuilder) -> Result<(), RegisterError> + Send>,
}

impl RegisterEntityTypeCommand {
    /// Create command from a HodeiEntityType implementor
    pub fn new<T: HodeiEntityType + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            registration: Box::new(|builder| {
                builder.register_entity::<T>()?;  // ← Tipo capturado en compile-time
                Ok(())
            }),
        }
    }
}
```

**Ventajas:**
- ✅ No hay capa de indirección
- ✅ Usa el tipo original (User, Group, etc.)
- ✅ El closure llama directamente a `builder.register_entity::<T>()`
- ✅ Funciona con el EngineBuilder existente
- ✅ Respeta arquitectura VSA

---

## 📦 hodei-policies: Features

### Feature 1: `register_entity_type`

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_entity_type/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**NO tiene `ports.rs`** - No depende de externos

#### `dto.rs`

```rust
use crate::internal::engine::builder::EngineBuilder;
use std::any::TypeId;

/// Command to register an entity type
///
/// This command captures the type T at compile-time using a closure,
/// avoiding unnecessary indirection layers.
pub struct RegisterEntityTypeCommand {
    /// TypeId for debugging and deduplication
    pub type_id: TypeId,
    
    /// Closure that registers the type in the builder
    /// Captures T at compile-time
    pub registration: Box<dyn FnOnce(&mut EngineBuilder) -> Result<(), RegisterEntityTypeError> + Send>,
}

impl RegisterEntityTypeCommand {
    /// Create a registration command from a HodeiEntityType implementor
    ///
    /// # Example
    ///
    /// ```rust
    /// use hodei_iam::internal::domain::User;
    /// 
    /// let command = RegisterEntityTypeCommand::new::<User>();
    /// ```
    pub fn new<T: kernel::HodeiEntityType + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            registration: Box::new(|builder| {
                builder.register_entity::<T>()
                    .map_err(|e| RegisterEntityTypeError::RegistrationFailed(e.to_string()))?;
                Ok(())
            }),
        }
    }
}

/// View of registered entity type
pub struct EntityTypeView {
    pub type_id: TypeId,
    pub registered: bool,
}
```

#### `error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterEntityTypeError {
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),
    
    #[error("Builder locked")]
    BuilderLocked,
}
```

#### `use_case.rs`

```rust
use super::dto::{RegisterEntityTypeCommand, EntityTypeView};
use super::error::RegisterEntityTypeError;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, instrument};

/// Use case for registering entity types in the schema builder
///
/// This use case maintains a reference to the shared EngineBuilder
/// and executes registration closures that capture types at compile-time.
pub struct RegisterEntityTypeUseCase {
    /// Shared reference to the engine builder
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterEntityTypeUseCase {
    /// Create new use case with shared builder
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    #[instrument(skip(self, command), fields(type_id = ?command.type_id))]
    pub async fn execute(
        &self,
        mut command: RegisterEntityTypeCommand,
    ) -> Result<EntityTypeView, RegisterEntityTypeError> {
        info!("Registering entity type");

        // Lock builder
        let mut builder = self.builder.lock().await;

        // Execute registration closure (calls builder.register_entity::<T>())
        (command.registration)(&mut builder)?;

        info!("Entity type registered successfully");

        Ok(EntityTypeView {
            type_id: command.type_id,
            registered: true,
        })
    }
}
```

#### `di.rs`

```rust
use super::use_case::RegisterEntityTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct RegisterEntityTypeUseCaseFactory;

impl RegisterEntityTypeUseCaseFactory {
    pub fn build(builder: Arc<Mutex<EngineBuilder>>) -> RegisterEntityTypeUseCase {
        RegisterEntityTypeUseCase::new(builder)
    }
}
```

---

### Feature 2: `register_action_type`

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_action_type/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `dto.rs`

```rust
use crate::internal::engine::builder::EngineBuilder;
use std::any::TypeId;

/// Command to register an action type
pub struct RegisterActionTypeCommand {
    pub action_id: TypeId,
    pub registration: Box<dyn FnOnce(&mut EngineBuilder) -> Result<(), RegisterActionTypeError> + Send>,
}

impl RegisterActionTypeCommand {
    /// Create a registration command from an ActionTrait implementor
    ///
    /// # Example
    ///
    /// ```rust
    /// use hodei_iam::internal::actions::CreateUserAction;
    /// 
    /// let command = RegisterActionTypeCommand::new::<CreateUserAction>();
    /// ```
    pub fn new<A: kernel::ActionTrait + 'static>() -> Self {
        Self {
            action_id: TypeId::of::<A>(),
            registration: Box::new(|builder| {
                builder.register_action_type::<A>()
                    .map_err(|e| RegisterActionTypeError::RegistrationFailed(e.to_string()))?;
                Ok(())
            }),
        }
    }
}

/// View of registered action type
pub struct ActionTypeView {
    pub action_id: TypeId,
    pub registered: bool,
}
```

#### `use_case.rs`

```rust
use super::dto::{RegisterActionTypeCommand, ActionTypeView};
use super::error::RegisterActionTypeError;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, instrument};

/// Use case for registering action types in the schema builder
pub struct RegisterActionTypeUseCase {
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterActionTypeUseCase {
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    #[instrument(skip(self, command), fields(action_id = ?command.action_id))]
    pub async fn execute(
        &self,
        mut command: RegisterActionTypeCommand,
    ) -> Result<ActionTypeView, RegisterActionTypeError> {
        info!("Registering action type");

        let mut builder = self.builder.lock().await;

        // Execute registration closure (calls builder.register_action_type::<A>())
        (command.registration)(&mut builder)?;

        info!("Action type registered successfully");

        Ok(ActionTypeView {
            action_id: command.action_id,
            registered: true,
        })
    }
}
```

---

### Feature 3: `build_schema`

**Estructura VSA:**
```
crates/hodei-policies/src/features/build_schema/
├── mod.rs
├── use_case.rs
├── ports.rs
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

/// Port for schema storage operations
#[async_trait]
pub trait SchemaStoragePort: Send + Sync {
    /// Save schema to persistent storage
    async fn save_schema(
        &self,
        schema_content: String,
        schema_hash: String,
    ) -> Result<PersistedSchemaView, BuildSchemaError>;

    /// Load latest schema from storage
    async fn load_latest_schema(
        &self,
    ) -> Result<Option<PersistedSchemaView>, BuildSchemaError>;
}
```

#### `dto.rs`

```rust
use chrono::{DateTime, Utc};

/// Command to build schema (no parameters needed)
pub struct BuildSchemaCommand;

/// View of the final schema (built and persisted)
pub struct SchemaView {
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub entity_count: usize,
    pub action_count: usize,
    pub created_at: DateTime<Utc>,
    pub was_persisted: bool,
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

#### `use_case.rs`

```rust
use super::dto::{BuildSchemaCommand, SchemaView, PersistedSchemaView};
use super::error::BuildSchemaError;
use super::ports::SchemaStoragePort;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, instrument, warn};
use sha2::{Sha256, Digest};
use chrono::Utc;

/// Use case for building and persisting the final Cedar schema
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

    #[instrument(skip(self, _command))]
    pub async fn execute(
        &self,
        _command: BuildSchemaCommand,
    ) -> Result<SchemaView, BuildSchemaError> {
        info!("Building final Cedar schema");

        // Get counts before consuming builder
        let entity_count;
        let action_count;
        {
            let builder_guard = self.builder.lock().await;
            entity_count = builder_guard.entity_count();
            action_count = builder_guard.action_count();
        }

        // Take ownership of builder to build schema
        let builder = Arc::try_unwrap(self.builder.clone())
            .map_err(|_| BuildSchemaError::BuilderInUse)?
            .into_inner();

        let schema = builder.build_schema()
            .map_err(|e| BuildSchemaError::SchemaGenerationError(e.to_string()))?;

        // Serialize to JSON
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

        // Compare with existing schema
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
        } else {
            info!("No existing schema, persisting for first time");
        }

        // Persist new schema (INTERNO)
        let persisted = self.storage.save_schema(schema_json, schema_hash).await?;

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
    }
}
```

---

## 📦 hodei-iam: Feature para Registro

### Feature: `register_iam_schema`

**Estructura VSA:**
```
crates/hodei-iam/src/features/register_iam_schema/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

**NO tiene `ports.rs`** - Usa directamente los use cases de hodei-policies

#### `dto.rs`

```rust
/// Command to register IAM schema (no parameters)
pub struct RegisterIamSchemaCommand;

/// View of registered IAM schema
pub struct IamSchemaView {
    pub entity_types_registered: Vec<String>,
    pub action_types_registered: Vec<String>,
}
```

#### `use_case.rs`

```rust
use super::dto::{RegisterIamSchemaCommand, IamSchemaView};
use super::error::RegisterIamSchemaError;
use crate::internal::domain::{User, Group, Account};
use crate::internal::actions::*;
use std::sync::Arc;
use tracing::{info, instrument};

// Import use cases from hodei-policies (NOT ports, use cases directly)
use hodei_policies::features::register_entity_type::{
    RegisterEntityTypeUseCase,
    dto::RegisterEntityTypeCommand,
};
use hodei_policies::features::register_action_type::{
    RegisterActionTypeUseCase,
    dto::RegisterActionTypeCommand,
};

/// Use case for registering all IAM types at application startup
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

    #[instrument(skip(self, _command))]
    pub async fn execute(
        &self,
        _command: RegisterIamSchemaCommand,
    ) -> Result<IamSchemaView, RegisterIamSchemaError> {
        info!("Registering IAM schema types");

        let mut entity_types = Vec::new();
        let mut action_types = Vec::new();

        // Register entity types using closures that capture the type
        self.register_entity::<User>(&mut entity_types, "Iam::User").await?;
        self.register_entity::<Group>(&mut entity_types, "Iam::Group").await?;
        self.register_entity::<Account>(&mut entity_types, "Iam::Account").await?;

        // Register action types using closures
        self.register_action::<CreateUserAction>(&mut action_types, "CreateUser").await?;
        self.register_action::<UpdateUserAction>(&mut action_types, "UpdateUser").await?;
        self.register_action::<DeleteUserAction>(&mut action_types, "DeleteUser").await?;
        self.register_action::<CreateGroupAction>(&mut action_types, "CreateGroup").await?;
        self.register_action::<AddToGroupAction>(&mut action_types, "AddToGroup").await?;
        self.register_action::<RemoveFromGroupAction>(&mut action_types, "RemoveFromGroup").await?;

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

    async fn register_entity<T: kernel::HodeiEntityType + 'static>(
        &self,
        registered: &mut Vec<String>,
        type_name: &str,
    ) -> Result<(), RegisterIamSchemaError> {
        // Create command with closure that captures T
        let command = RegisterEntityTypeCommand::new::<T>();

        // Execute - the closure inside will call builder.register_entity::<T>()
        self.entity_registrar.execute(command).await
            .map_err(|e| RegisterIamSchemaError::EntityRegistrationFailed(e.to_string()))?;

        registered.push(type_name.to_string());
        Ok(())
    }

    async fn register_action<A: kernel::ActionTrait + 'static>(
        &self,
        registered: &mut Vec<String>,
        action_name: &str,
    ) -> Result<(), RegisterIamSchemaError> {
        // Create command with closure that captures A
        let command = RegisterActionTypeCommand::new::<A>();

        // Execute - the closure inside will call builder.register_action_type::<A>()
        self.action_registrar.execute(command).await
            .map_err(|e| RegisterIamSchemaError::ActionRegistrationFailed(e.to_string()))?;

        registered.push(action_name.to_string());
        Ok(())
    }
}
```

#### `di.rs`

```rust
use super::use_case::RegisterIamSchemaUseCase;
use hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase;
use hodei_policies::features::register_action_type::RegisterActionTypeUseCase;
use std::sync::Arc;

pub struct RegisterIamSchemaUseCaseFactory;

impl RegisterIamSchemaUseCaseFactory {
    pub fn build(
        entity_registrar: Arc<RegisterEntityTypeUseCase>,
        action_registrar: Arc<RegisterActionTypeUseCase>,
    ) -> RegisterIamSchemaUseCase {
        RegisterIamSchemaUseCase::new(entity_registrar, action_registrar)
    }
}
```

---

## 🚀 Flujo de Arranque en main.rs

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use hodei_policies::internal::engine::builder::EngineBuilder;
use hodei_policies::features::{
    register_entity_type::RegisterEntityTypeUseCase,
    register_action_type::RegisterActionTypeUseCase,
    build_schema::{BuildSchemaUseCase, dto::BuildSchemaCommand},
};
use hodei_iam::features::register_iam_schema::{
    RegisterIamSchemaUseCase,
    dto::RegisterIamSchemaCommand,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Inicializar infraestructura
    let db = initialize_surrealdb().await?;
    
    // 2. Crear EngineBuilder compartido (INTERNO a hodei-policies)
    let builder = Arc::new(Mutex::new(EngineBuilder::new()));
    
    // 3. Crear use cases de hodei-policies para registro
    let register_entity_uc = Arc::new(RegisterEntityTypeUseCase::new(builder.clone()));
    let register_action_uc = Arc::new(RegisterActionTypeUseCase::new(builder.clone()));
    
    info!("Registering schema types from all bounded contexts");
    
    // 4. Registrar tipos de IAM
    let register_iam_uc = RegisterIamSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_iam_uc.execute(RegisterIamSchemaCommand).await?;
    
    // 5. Registrar tipos de Organizations
    let register_orgs_uc = hodei_organizations::features::register_org_schema::RegisterOrgSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_orgs_uc.execute(
        hodei_organizations::features::register_org_schema::dto::RegisterOrgSchemaCommand
    ).await?;
    
    // 6. Registrar tipos de Artifacts
    let register_artifacts_uc = hodei_artifacts::features::register_artifact_schema::RegisterArtifactSchemaUseCase::new(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    register_artifacts_uc.execute(
        hodei_artifacts::features::register_artifact_schema::dto::RegisterArtifactSchemaCommand
    ).await?;
    
    info!("All types registered. Building schema...");
    
    // 7. Construir y persistir schema
    let schema_storage = Arc::new(
        hodei_policies::infrastructure::surreal::SurrealSchemaStorageAdapter::new(db.clone())
    );
    
    let build_schema_uc = BuildSchemaUseCase::new(builder, schema_storage);
    let schema_view = build_schema_uc.execute(BuildSchemaCommand).await?;
    
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

### 1. Sin Capa de Indirección
```rust
// ❌ ANTES (innecesario)
let type_info = EntityTypeInfo::from_type::<User>();  // Extrae info manualmente
let command = RegisterEntityTypeCommand { type_info };

// ✅ AHORA (directo)
let command = RegisterEntityTypeCommand::new::<User>();  // Closure captura el tipo
```

### 2. Usa el EngineBuilder Existente
```rust
// El closure llama directamente al método existente
registration: Box::new(|builder| {
    builder.register_entity::<T>()  // ← Método existente del EngineBuilder
})
```

### 3. Type-Safe en Compile-Time
```rust
// El tipo T se captura en compile-time
RegisterEntityTypeCommand::new::<User>();  // ← User está en el tipo del closure
```

### 4. Respeta Arquitectura VSA
- ✅ Todo via features con use cases
- ✅ EngineBuilder completamente interno
- ✅ Bounded contexts desacoplados

### 5. Flexible y Extensible
```rust
// Fácil agregar más tipos
command1 = RegisterEntityTypeCommand::new::<User>();
command2 = RegisterEntityTypeCommand::new::<Group>();
command3 = RegisterEntityTypeCommand::new::<Document>();
```

---

## 📊 Comparación

### ❌ Con EntityTypeInfo (innecesario)

```rust
// Paso 1: Extraer info manualmente
pub struct EntityTypeInfo {
    pub service_name: String,
    pub resource_type_name: String,
    pub attributes_schema: HashMap<AttributeName, AttributeType>,
}

impl EntityTypeInfo {
    pub fn from_type<T: HodeiEntityType>() -> Self {
        Self {
            service_name: T::service_name().as_str().to_string(),
            resource_type_name: T::resource_type_name().as_str().to_string(),
            attributes_schema: T::attributes_schema(),
        }
    }
}

// Paso 2: Crear comando con la info
let type_info = EntityTypeInfo::from_type::<User>();
let command = RegisterEntityTypeCommand { type_info };

// Paso 3: El use case regenera el fragment desde la info
let fragment = generate_fragment_from_info(&command.type_info);
builder.register_fragment(fragment);
```

### ✅ Con Closures (directo)

```rust
// Paso 1: Crear comando con closure que captura el tipo
let command = RegisterEntityTypeCommand::new::<User>();

// Paso 2: El use case ejecuta el closure
(command.registration)(&mut builder);
// ↑ Esto llama a builder.register_entity::<User>() directamente
```

**Diferencia:**
- ❌ Con EntityTypeInfo: extraer → serializar → deserializar → regenerar
- ✅ Con Closures: capturar tipo → ejecutar directamente

---

## ⏱️ Tiempo Estimado

| Tarea | Tiempo |
|-------|--------|
| Feature register_entity_type | 1.5h |
| Feature register_action_type | 1.5h |
| Feature build_schema | 2h |
| Feature load_schema | 1.5h |
| Modificar validate_policy | 1h |
| Modificar evaluate_policies | 1h |
| Adaptador SurrealDB | 1.5h |
| Feature register_iam_schema | 1h |
| Módulo actions IAM | 2h |
| Integración main.rs | 2h |
| **TOTAL** | **~15 horas** |

---

## 🎯 Criterios de Aceptación

- [ ] EngineBuilder NUNCA se expone fuera de hodei-policies
- [ ] Comandos usan closures, NO EntityTypeInfo/ActionTypeInfo
- [ ] Closures capturan tipos en compile-time
- [ ] Closures llaman directamente a builder.register_entity::<T>()
- [ ] Todo via features VSA con use cases
- [ ] Builder compartido via Arc<Mutex<>>
- [ ] build_schema consume el builder (solo una vez)
- [ ] Persistencia interna a build_schema
- [ ] Todos los tests pasan
- [ ] Código compila sin warnings

---

## 🚀 Estado

✅ **ARQUITECTURA DEFINITIVA - LISTA PARA IMPLEMENTACIÓN**

Esta es la solución más simple, directa y correcta. Sin capas de indirección innecesarias.