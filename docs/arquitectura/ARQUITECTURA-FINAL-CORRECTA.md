# Arquitectura FINAL Correcta: Schema Management con Features VSA

## 🎯 Principio Fundamental

**El `EngineBuilder` es COMPLETAMENTE INTERNO a `hodei-policies`**

- ❌ NUNCA se expone directamente
- ❌ NUNCA se pasa a otros bounded contexts
- ✅ TODO se hace a través de **features VSA** con **use cases** y **ports**

---

## 🏗️ Arquitectura General

```
┌─────────────────────────────────────────────────────────────┐
│                    main.rs (Composition Root)               │
└────────────────────────────┬────────────────────────────────┘
                             │
                             │ 1. Inicializa use cases
                             ▼
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  hodei-iam   │    │hodei-orgs    │    │hodei-artifacts│
│              │    │              │    │              │
│register_iam_ │    │register_org_ │    │register_art_ │
│schema UC     │    │schema UC     │    │schema UC     │
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                    │
       │ RegisterEntityTypePort (async)         │
       │ RegisterActionTypePort (async)         │
       └───────────────────┼────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              hodei-policies (Features VSA)                  │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Feature: register_entity_type                      │   │
│  │  ├── RegisterEntityTypeUseCase                      │   │
│  │  │   └── Accede a EngineBuilder interno             │   │
│  │  └── Expone: RegisterEntityTypePort                 │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Feature: register_action_type                      │   │
│  │  ├── RegisterActionTypeUseCase                      │   │
│  │  │   └── Accede a EngineBuilder interno             │   │
│  │  └── Expone: RegisterActionTypePort                 │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Feature: build_schema                              │   │
│  │  ├── BuildSchemaUseCase                             │   │
│  │  │   ├── Toma EngineBuilder (consume)               │   │
│  │  │   ├── builder.build_schema()                     │   │
│  │  │   └── Persiste automáticamente (interno)         │   │
│  │  └── Port: SchemaStoragePort                        │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌──────────────────────────────────────┐                  │
│  │  EngineBuilder (INTERNO)             │                  │
│  │  - Compartido entre use cases        │                  │
│  │  - Arc<Mutex<EngineBuilder>>         │                  │
│  │  - NUNCA se expone fuera             │                  │
│  └──────────────────────────────────────┘                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 📦 hodei-policies: Features y Estructura

### Feature 1: `register_entity_type` (NUEVO)

**Objetivo:** Registrar un entity type en el builder interno

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_entity_type/
├── mod.rs
├── use_case.rs
├── ports.rs          # NINGUNO (no depende de externos)
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `dto.rs`

```rust
use kernel::{AttributeName, AttributeType};
use std::collections::HashMap;

/// Information extracted from a HodeiEntityType implementor
#[derive(Debug, Clone)]
pub struct EntityTypeInfo {
    pub service_name: String,
    pub resource_type_name: String,
    pub attributes_schema: HashMap<AttributeName, AttributeType>,
}

impl EntityTypeInfo {
    /// Extract type info from a HodeiEntityType implementor
    pub fn from_type<T: kernel::HodeiEntityType>() -> Self {
        Self {
            service_name: T::service_name().as_str().to_string(),
            resource_type_name: T::resource_type_name().as_str().to_string(),
            attributes_schema: T::attributes_schema(),
        }
    }
}

/// Command to register an entity type
pub struct RegisterEntityTypeCommand {
    pub type_info: EntityTypeInfo,
}

/// View of registered entity type
pub struct EntityTypeView {
    pub entity_type_name: String,
    pub registered: bool,
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
/// and registers entity types as they come in from different bounded contexts.
pub struct RegisterEntityTypeUseCase {
    /// Shared reference to the engine builder
    builder: Arc<Mutex<EngineBuilder>>,
}

impl RegisterEntityTypeUseCase {
    /// Create new use case with shared builder
    pub fn new(builder: Arc<Mutex<EngineBuilder>>) -> Self {
        Self { builder }
    }

    #[instrument(skip(self, command), fields(
        entity_type = format!("{}::{}", 
            command.type_info.service_name, 
            command.type_info.resource_type_name
        )
    ))]
    pub async fn execute(
        &self,
        command: RegisterEntityTypeCommand,
    ) -> Result<EntityTypeView, RegisterEntityTypeError> {
        let entity_type_name = format!(
            "{}::{}",
            command.type_info.service_name,
            command.type_info.resource_type_name
        );

        info!("Registering entity type: {}", entity_type_name);

        // Lock builder
        let mut builder = self.builder.lock().await;

        // Generate Cedar schema fragment from type info
        let schema_fragment = self.generate_schema_fragment(&command.type_info)?;

        // Register in builder (usando método interno que acepte string)
        // O mejor: agregar el fragment directamente al builder
        builder.register_entity_fragment(
            entity_type_name.clone(),
            schema_fragment,
        )?;

        info!("Entity type registered successfully");

        Ok(EntityTypeView {
            entity_type_name,
            registered: true,
        })
    }

    /// Generate Cedar schema fragment from entity type info
    fn generate_schema_fragment(
        &self,
        type_info: &EntityTypeInfo,
    ) -> Result<String, RegisterEntityTypeError> {
        let mut fragment = format!("entity {} {{\n", type_info.resource_type_name);

        for (attr_name, attr_type) in &type_info.attributes_schema {
            let cedar_type = match attr_type {
                kernel::AttributeType::String => "String",
                kernel::AttributeType::Long => "Long",
                kernel::AttributeType::Boolean => "Bool",
                kernel::AttributeType::Set => "Set<String>",
                kernel::AttributeType::Record => "Record",
                kernel::AttributeType::EntityOrCommon => "Entity",
            };
            fragment.push_str(&format!("  {}: {},\n", attr_name.as_str(), cedar_type));
        }

        fragment.push_str("}");
        Ok(fragment)
    }
}
```

---

### Feature 2: `register_action_type` (NUEVO)

**Objetivo:** Registrar un action type en el builder interno

**Estructura VSA:**
```
crates/hodei-policies/src/features/register_action_type/
├── mod.rs
├── use_case.rs
├── ports.rs          # NINGUNO
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `dto.rs`

```rust
/// Information extracted from an ActionTrait implementor
#[derive(Debug, Clone)]
pub struct ActionTypeInfo {
    pub action_name: String,
    pub service_name: String,
    pub applies_to_principal: String,
    pub applies_to_resource: String,
}

impl ActionTypeInfo {
    /// Extract action info from an ActionTrait implementor
    pub fn from_action<A: kernel::ActionTrait>() -> Self {
        Self {
            action_name: A::name().to_string(),
            service_name: A::service_name().as_str().to_string(),
            applies_to_principal: A::applies_to_principal(),
            applies_to_resource: A::applies_to_resource(),
        }
    }
}

/// Command to register an action type
pub struct RegisterActionTypeCommand {
    pub action_info: ActionTypeInfo,
}

/// View of registered action type
pub struct ActionTypeView {
    pub action_name: String,
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

    #[instrument(skip(self, command), fields(action_name = %command.action_info.action_name))]
    pub async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<ActionTypeView, RegisterActionTypeError> {
        info!("Registering action type: {}", command.action_info.action_name);

        let mut builder = self.builder.lock().await;

        // Generate Cedar action fragment
        let action_fragment = format!(
            r#"action "{}" appliesTo {{
    principal: [{}],
    resource: [{}]
}};"#,
            command.action_info.action_name,
            command.action_info.applies_to_principal,
            command.action_info.applies_to_resource
        );

        builder.register_action_fragment(action_fragment)?;

        info!("Action type registered successfully");

        Ok(ActionTypeView {
            action_name: command.action_info.action_name,
            registered: true,
        })
    }
}
```

---

### Feature 3: `build_schema` (NUEVO)

**Objetivo:** Construir el schema final y persistirlo automáticamente

**Estructura VSA:**
```
crates/hodei-policies/src/features/build_schema/
├── mod.rs
├── use_case.rs
├── ports.rs          # SchemaStoragePort
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
///
/// This use case:
/// 1. Takes the populated EngineBuilder
/// 2. Calls build_schema() to generate Cedar schema
/// 3. Compares with persisted schema
/// 4. Persists only if changed (INTERNAL to use case)
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

        // 1. Get counts before building
        let builder_guard = self.builder.lock().await;
        let entity_count = builder_guard.entity_count();
        let action_count = builder_guard.action_count();
        drop(builder_guard);

        // 2. Take builder to build schema (requires ownership)
        // NOTE: Esto consume el builder, así que debe ser el último paso
        let builder = Arc::try_unwrap(self.builder.clone())
            .map_err(|_| BuildSchemaError::BuilderInUse)?
            .into_inner();

        let schema = builder.build_schema()
            .map_err(|e| BuildSchemaError::SchemaGenerationError(e.to_string()))?;

        // 3. Serialize to JSON for persistence
        let schema_json = serde_json::to_string_pretty(&schema)
            .map_err(|e| BuildSchemaError::SerializationError(e.to_string()))?;

        // 4. Calculate hash
        let mut hasher = Sha256::new();
        hasher.update(&schema_json);
        let schema_hash = format!("{:x}", hasher.finalize());

        info!(
            entity_count = entity_count,
            action_count = action_count,
            schema_hash = %schema_hash,
            "Schema built successfully"
        );

        // 5. Compare with existing schema
        let existing = self.storage.load_latest_schema().await?;
        
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

        // 6. Persist new schema (INTERNO - no feature separada)
        let persisted = self
            .storage
            .save_schema(schema_json, schema_hash)
            .await?;

        info!(version = %persisted.version, "Schema persisted successfully");

        Ok(SchemaView {
            schema_content: persisted.schema_content,
            schema_hash: persisted.schema_hash,
            version: persisted.version,
            entity_count,
            action_count,
            created_at: persisted.created_at,
            was_persisted,
        })
    }
}
```

---

## 📦 hodei-iam: Feature para Registro

### Feature: `register_iam_schema` (NUEVO)

**Objetivo:** Registrar todos los tipos IAM usando los ports de hodei-policies

**Estructura VSA:**
```
crates/hodei-iam/src/features/register_iam_schema/
├── mod.rs
├── use_case.rs
├── ports.rs          # Re-exporta ports de hodei-policies
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### `ports.rs`

```rust
use async_trait::async_trait;
use super::error::RegisterIamSchemaError;

// Re-exportar tipos de hodei-policies
pub use hodei_policies::features::register_entity_type::dto::EntityTypeInfo;
pub use hodei_policies::features::register_action_type::dto::ActionTypeInfo;

/// Port for registering entity types (from hodei-policies)
#[async_trait]
pub trait RegisterEntityTypePort: Send + Sync {
    async fn register_entity_type(
        &self,
        command: hodei_policies::features::register_entity_type::dto::RegisterEntityTypeCommand,
    ) -> Result<
        hodei_policies::features::register_entity_type::dto::EntityTypeView,
        RegisterIamSchemaError,
    >;
}

/// Port for registering action types (from hodei-policies)
#[async_trait]
pub trait RegisterActionTypePort: Send + Sync {
    async fn register_action_type(
        &self,
        command: hodei_policies::features::register_action_type::dto::RegisterActionTypeCommand,
    ) -> Result<
        hodei_policies::features::register_action_type::dto::ActionTypeView,
        RegisterIamSchemaError,
    >;
}
```

#### `use_case.rs`

```rust
use super::dto::{RegisterIamSchemaCommand, IamSchemaView};
use super::error::RegisterIamSchemaError;
use super::ports::{RegisterEntityTypePort, RegisterActionTypePort, EntityTypeInfo, ActionTypeInfo};
use crate::internal::domain::{User, Group, Account};
use crate::internal::actions::*;
use std::sync::Arc;
use tracing::{info, instrument};

/// Use case for registering all IAM types at application startup
///
/// This use case uses the ports from hodei-policies to register
/// all IAM entity types and action types.
pub struct RegisterIamSchemaUseCase<ET, AT>
where
    ET: RegisterEntityTypePort,
    AT: RegisterActionTypePort,
{
    entity_registrar: Arc<ET>,
    action_registrar: Arc<AT>,
}

impl<ET, AT> RegisterIamSchemaUseCase<ET, AT>
where
    ET: RegisterEntityTypePort,
    AT: RegisterActionTypePort,
{
    pub fn new(entity_registrar: Arc<ET>, action_registrar: Arc<AT>) -> Self {
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

        // Register entity types
        self.register_entity_type::<User>(&mut entity_types).await?;
        self.register_entity_type::<Group>(&mut entity_types).await?;
        self.register_entity_type::<Account>(&mut entity_types).await?;

        // Register action types
        self.register_action::<CreateUserAction>(&mut action_types).await?;
        self.register_action::<UpdateUserAction>(&mut action_types).await?;
        self.register_action::<DeleteUserAction>(&mut action_types).await?;
        self.register_action::<CreateGroupAction>(&mut action_types).await?;
        self.register_action::<AddToGroupAction>(&mut action_types).await?;
        self.register_action::<RemoveFromGroupAction>(&mut action_types).await?;

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

    async fn register_entity_type<T: kernel::HodeiEntityType>(
        &self,
        registered: &mut Vec<String>,
    ) -> Result<(), RegisterIamSchemaError> {
        // Extract type info
        let type_info = EntityTypeInfo::from_type::<T>();
        let type_name = format!("{}::{}", type_info.service_name, type_info.resource_type_name);

        // Call port to register
        let command = hodei_policies::features::register_entity_type::dto::RegisterEntityTypeCommand {
            type_info,
        };

        self.entity_registrar.register_entity_type(command).await?;
        registered.push(type_name);
        Ok(())
    }

    async fn register_action<A: kernel::ActionTrait>(
        &self,
        registered: &mut Vec<String>,
    ) -> Result<(), RegisterIamSchemaError> {
        // Extract action info
        let action_info = ActionTypeInfo::from_action::<A>();
        let action_name = action_info.action_name.clone();

        // Call port to register
        let command = hodei_policies::features::register_action_type::dto::RegisterActionTypeCommand {
            action_info,
        };

        self.action_registrar.register_action_type(command).await?;
        registered.push(action_name);
        Ok(())
    }
}
```

---

## 🚀 Flujo de Arranque en main.rs

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use hodei_policies::internal::engine::builder::EngineBuilder;
use hodei_policies::features::{register_entity_type, register_action_type, build_schema};
use hodei_iam::features::register_iam_schema;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Inicializar infraestructura
    let db = initialize_surrealdb().await?;
    
    // 2. Crear EngineBuilder compartido (INTERNO a hodei-policies)
    let builder = Arc::new(Mutex::new(EngineBuilder::new()));
    
    // 3. Crear use cases de hodei-policies para registro
    let register_entity_uc = Arc::new(
        register_entity_type::RegisterEntityTypeUseCase::new(builder.clone())
    );
    
    let register_action_uc = Arc::new(
        register_action_type::RegisterActionTypeUseCase::new(builder.clone())
    );
    
    info!("Registering schema types from all bounded contexts");
    
    // 4. Registrar tipos de IAM (usa los use cases como ports)
    let register_iam_uc = register_iam_schema::di::RegisterIamSchemaUseCaseFactory::build(
        register_entity_uc.clone(),  // ← Use case implementa el port
        register_action_uc.clone(),
    );
    
    register_iam_uc.execute(
        register_iam_schema::dto::RegisterIamSchemaCommand
    ).await?;
    
    // 5. Registrar tipos de Organizations
    let register_orgs_uc = hodei_organizations::features::register_org_schema::di::RegisterOrgSchemaUseCaseFactory::build(
        register_entity_uc.clone(),
        register_action_uc.clone(),
    );
    
    register_orgs_uc.execute(
        hodei_organizations::features::register_org_schema::dto::RegisterOrgSchemaCommand
    ).await?;
    
    // 6. Registrar tipos de Artifacts
    let register_artifacts_uc = hodei_artifacts::features::register_artifact_schema::di::RegisterArtifactSchemaUseCaseFactory::build(
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
    
    let build_schema_uc = build_schema::BuildSchemaUseCase::new(
        builder.clone(),
        schema_storage,
    );
    
    let schema_view = build_schema_uc.execute(
        build_schema::dto::BuildSchemaCommand
    ).await?;
    
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

## ✅ Puntos Clave de la Arquitectura

### 1. EngineBuilder es INTERNO
- ✅ Solo existe dentro de `hodei-policies`
- ✅ Se comparte entre use cases via `Arc<Mutex<EngineBuilder>>`
- ✅ NUNCA se expone fuera del crate

### 2. Todo via Features VSA
- ✅ `register_entity_type` - Feature completa con use case
- ✅ `register_action_type` - Feature completa con use case
- ✅ `build_schema` - Feature completa con use case

### 3. Bounded Contexts usan Ports
- ✅ hodei-iam depende de `RegisterEntityTypePort` y `RegisterActionTypePort`
- ✅ Los use cases de hodei-policies implementan estos ports
- ✅ La composition root (main.rs) hace la DI

### 4. Builder se consume al final
- ✅ Los registros van acumulando en el builder compartido
- ✅ `build_schema` toma ownership y consume el builder
- ✅ Solo se puede llamar `build_schema` una vez

### 5. Persistencia es Interna
- ✅ `BuildSchemaUseCase` persiste automáticamente
- ✅ Solo persiste si el schema cambió
- ✅ No hay feature separada `persist_schema`

---

## 📊 Resumen de Features

### hodei-policies (4 features)

1. ✅ `register_entity_type` - Use case que accede a builder interno
2. ✅ `register_action_type` - Use case que accede a builder interno
3. ✅ `build_schema` - Construye y persiste automáticamente
4. ✅ `load_schema` - Carga schema para validate/evaluate

### hodei-iam (1 feature)

1. ✅ `register_iam_schema` - Usa ports de hodei-policies

### Otros BCs (1 feature cada uno)

1. ✅ `register_org_schema` en hodei-organizations
2. ✅ `register_artifact_schema` en hodei-artifacts

---

## ⏱️ Tiempo Estimado

| Tarea | Tiempo |
|-------|--------|
| Feature register_entity_type | 2 horas |
| Feature register_action_type | 2 horas |
| Feature build_schema | 2 horas |
| Feature load_schema | 1.5 horas |
| Modificar validate_policy | 1 hora |
| Modificar evaluate_policies | 1 hora |
| Adaptador SurrealDB | 1.5 horas |
| Feature register_iam_schema | 1.5 horas |
| Módulo actions IAM | 2 horas |
| Integración main.rs | 2 horas |
| **TOTAL** | **~16.5 horas** |

---

## 🎯 Criterios de Aceptación

- [ ] EngineBuilder NUNCA se expone fuera de hodei-policies
- [ ] Todo se hace via features VSA con use cases
- [ ] Bounded contexts usan ports, no implementaciones
- [ ] Builder se comparte via Arc<Mutex<>> entre use cases
- [ ] build_schema consume el builder (solo se llama una vez)
- [ ] Persistencia es interna a build_schema
- [ ] Schema se carga para validate/evaluate
- [ ] Todos los tests pasan
- [ ] Código compila sin warnings

---

**Estado:** ✅ **ARQUITECTURA FINAL CORRECTA Y LISTA**