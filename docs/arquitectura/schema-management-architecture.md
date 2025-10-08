# Arquitectura de Schema Management para Cedar Authorization Engine

## Resumen Ejecutivo

Este documento especifica la arquitectura completa para el **schema management dinámico** del Cedar Authorization Engine en Hodei Artifacts. El schema se construye dinámicamente al arranque de la aplicación registrando entity types y action types desde todos los bounded contexts, se persiste, y se reutiliza en reinicios si no hay cambios.

## Principios Arquitectónicos

### 1. Todo a través de Features VSA
- ❌ **NO** usar servicios o implementaciones directamente
- ✅ **SÍ** usar ports (traits) y use cases
- ✅ Cada operación es una feature completa con estructura VSA

### 2. Inversión de Dependencias
- hodei-iam depende de **ports** de hodei-policies, no de implementaciones
- La composition root (main.rs) inyecta implementaciones concretas

### 3. Schema como Artefacto de Primera Clase
- El schema se genera dinámicamente
- Se persiste en SurrealDB
- Se versiona y se compara en cada arranque
- Se usa como fuente de verdad para validación Cedar

## Arquitectura General

```
┌─────────────────────────────────────────────────────────────────┐
│                    Application Startup                          │
│                      (main.rs)                                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ 1. Inicializar SchemaBuilder
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│              hodei-policies::SchemaBuilder                      │
│              (Singleton durante arranque)                       │
└────────────────────────────┬────────────────────────────────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  hodei-iam   │    │hodei-orgs    │    │hodei-artifacts│
│              │    │              │    │              │
│register_iam_ │    │register_org_ │    │register_art_ │
│schema        │    │schema        │    │schema        │
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                    │
       │ RegisterEntityTypePort                 │
       │ RegisterActionTypePort                 │
       └───────────────────┼────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│   hodei-policies::features::build_schema::BuildSchemaUseCase   │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ Schema generado
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ hodei-policies::features::persist_schema::PersistSchemaUseCase │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             │ Guardar en SurrealDB
                             ▼
                    [cedar_schemas table]
```

## hodei-policies: Features y Ports Necesarios

### Feature 1: `evaluate_policies` (REFACTORIZAR)

**Problema Actual:** hodei-iam usa directamente `EvaluatePoliciesUseCase`

**Solución:** Crear port para evaluación

#### Archivos Afectados

**`crates/hodei-policies/src/features/evaluate_policies/ports.rs`** (NUEVO)

```rust
use async_trait::async_trait;
use super::dto::{EvaluatePoliciesCommand, EvaluationDecision};
use super::error::EvaluatePoliciesError;

/// Port for evaluating authorization policies
///
/// This port abstracts the policy evaluation logic, allowing bounded contexts
/// to evaluate Cedar policies without depending on concrete implementations.
///
/// # Segregation (ISP)
///
/// This port provides only evaluation capability. Other operations
/// (validation, schema management) are in separate ports.
#[async_trait]
pub trait EvaluatePoliciesPort: Send + Sync {
    /// Evaluate an authorization request against policies
    ///
    /// # Arguments
    ///
    /// * `command` - Evaluation command with request, policies, and entities
    ///
    /// # Returns
    ///
    /// Authorization decision (Allow/Deny) with reasoning
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError>;
}
```

**`crates/hodei-policies/src/features/evaluate_policies/use_case.rs`** (MODIFICAR)

```rust
// Agregar implementación del port
#[async_trait]
impl EvaluatePoliciesPort for EvaluatePoliciesUseCase {
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        self.execute(command).await
    }
}
```

**`crates/hodei-policies/src/features/evaluate_policies/mod.rs`** (MODIFICAR)

```rust
pub mod ports; // Agregar esta línea

// Re-exports
pub use ports::EvaluatePoliciesPort;
pub use use_case::EvaluatePoliciesUseCase;
```

---

### Feature 2: `register_entity_type` (NUEVO)

**Objetivo:** Permitir que bounded contexts registren sus entity types (User, Group, Document, etc.)

#### Estructura VSA

```
crates/hodei-policies/src/features/register_entity_type/
├── mod.rs
├── use_case.rs
├── ports.rs
├── dto.rs
├── error.rs
├── di.rs
├── mocks.rs
└── use_case_test.rs
```

#### Contenido de Archivos

**`ports.rs`**

```rust
use async_trait::async_trait;
use kernel::HodeiEntityType;
use super::error::RegisterEntityTypeError;

/// Port for accessing the schema builder to register entity types
///
/// This port follows ISP by providing only entity type registration.
#[async_trait]
pub trait SchemaBuilderPort: Send + Sync {
    /// Register an entity type in the schema builder
    ///
    /// # Arguments
    ///
    /// * `entity_type_name` - Fully qualified type name (e.g., "Iam::User")
    /// * `schema_fragment` - Cedar schema fragment for this entity
    ///
    /// # Errors
    ///
    /// - DuplicateEntityType: Entity already registered
    /// - InvalidSchemaFragment: Malformed schema
    async fn register_entity_type(
        &self,
        entity_type_name: String,
        schema_fragment: String,
    ) -> Result<(), RegisterEntityTypeError>;
}
```

**`dto.rs`**

```rust
use kernel::HodeiEntityType;

/// Command to register an entity type
pub struct RegisterEntityTypeCommand {
    /// The entity type implementing HodeiEntityType
    /// We use type name and schema because we can't store trait objects easily
    pub entity_type_name: String,
    pub service_name: String,
    pub resource_type_name: String,
    pub attributes_schema: std::collections::HashMap<
        kernel::AttributeName,
        kernel::AttributeType,
    >,
}

impl RegisterEntityTypeCommand {
    /// Create command from a HodeiEntityType implementor
    pub fn from_type<T: HodeiEntityType>() -> Self {
        Self {
            entity_type_name: format!(
                "{}::{}",
                T::service_name().as_str(),
                T::resource_type_name().as_str()
            ),
            service_name: T::service_name().as_str().to_string(),
            resource_type_name: T::resource_type_name().as_str().to_string(),
            attributes_schema: T::attributes_schema(),
        }
    }
}

/// View of registered entity type (for confirmation)
pub struct EntityTypeView {
    pub entity_type_name: String,
    pub schema_fragment: String,
}
```

**`use_case.rs`**

```rust
use super::dto::{RegisterEntityTypeCommand, EntityTypeView};
use super::error::RegisterEntityTypeError;
use super::ports::SchemaBuilderPort;
use std::sync::Arc;
use tracing::{info, instrument};

/// Use case for registering entity types in the Cedar schema
pub struct RegisterEntityTypeUseCase<SB>
where
    SB: SchemaBuilderPort,
{
    schema_builder: Arc<SB>,
}

impl<SB> RegisterEntityTypeUseCase<SB>
where
    SB: SchemaBuilderPort,
{
    pub fn new(schema_builder: Arc<SB>) -> Self {
        Self { schema_builder }
    }

    #[instrument(skip(self, command), fields(entity_type = %command.entity_type_name))]
    pub async fn execute(
        &self,
        command: RegisterEntityTypeCommand,
    ) -> Result<EntityTypeView, RegisterEntityTypeError> {
        info!("Registering entity type: {}", command.entity_type_name);

        // Generate Cedar schema fragment from entity type info
        let schema_fragment = self.generate_schema_fragment(&command)?;

        // Register in builder
        self.schema_builder
            .register_entity_type(
                command.entity_type_name.clone(),
                schema_fragment.clone(),
            )
            .await?;

        info!("Entity type registered successfully");

        Ok(EntityTypeView {
            entity_type_name: command.entity_type_name,
            schema_fragment,
        })
    }

    fn generate_schema_fragment(
        &self,
        command: &RegisterEntityTypeCommand,
    ) -> Result<String, RegisterEntityTypeError> {
        // Generate Cedar DSL schema fragment
        let mut fragment = format!("entity {} {{\n", command.resource_type_name);

        for (attr_name, attr_type) in &command.attributes_schema {
            let cedar_type = match attr_type {
                kernel::AttributeType::String => "String",
                kernel::AttributeType::Long => "Long",
                kernel::AttributeType::Boolean => "Bool",
                kernel::AttributeType::Set => "Set<String>", // Simplified
                kernel::AttributeType::Record => "Record", // Simplified
                kernel::AttributeType::EntityOrCommon => "Entity",
            };
            fragment.push_str(&format!("  {}: {},\n", attr_name.as_str(), cedar_type));
        }

        fragment.push_str("}");
        Ok(fragment)
    }
}
```

**`error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterEntityTypeError {
    #[error("Entity type already registered: {0}")]
    DuplicateEntityType(String),

    #[error("Invalid schema fragment: {0}")]
    InvalidSchemaFragment(String),

    #[error("Schema builder error: {0}")]
    SchemaBuilderError(String),
}
```

---

### Feature 3: `register_action_type` (NUEVO)

**Objetivo:** Permitir que bounded contexts registren sus action types

#### Estructura VSA

```
crates/hodei-policies/src/features/register_action_type/
├── mod.rs
├── use_case.rs
├── ports.rs        # Puede reutilizar SchemaBuilderPort con método adicional
├── dto.rs
├── error.rs
├── di.rs
├── mocks.rs
└── use_case_test.rs
```

#### Contenido de Archivos

**`ports.rs`**

```rust
use async_trait::async_trait;
use super::error::RegisterActionTypeError;

/// Port for registering action types in the schema builder
#[async_trait]
pub trait ActionRegistrarPort: Send + Sync {
    /// Register an action type in the schema builder
    ///
    /// # Arguments
    ///
    /// * `action_name` - Action name (e.g., "CreateUser")
    /// * `applies_to_principal` - Principal type (e.g., "Iam::User")
    /// * `applies_to_resource` - Resource type (e.g., "Iam::Group")
    async fn register_action_type(
        &self,
        action_name: String,
        applies_to_principal: String,
        applies_to_resource: String,
    ) -> Result<(), RegisterActionTypeError>;
}
```

**`dto.rs`**

```rust
use kernel::ActionTrait;

/// Command to register an action type
pub struct RegisterActionTypeCommand {
    pub action_name: String,
    pub service_name: String,
    pub applies_to_principal: String,
    pub applies_to_resource: String,
}

impl RegisterActionTypeCommand {
    /// Create command from an ActionTrait implementor
    pub fn from_action<A: ActionTrait>() -> Self {
        Self {
            action_name: A::name().to_string(),
            service_name: A::service_name().as_str().to_string(),
            applies_to_principal: A::applies_to_principal(),
            applies_to_resource: A::applies_to_resource(),
        }
    }
}

/// View of registered action type
pub struct ActionTypeView {
    pub action_name: String,
    pub schema_fragment: String,
}
```

**`use_case.rs`**

```rust
use super::dto::{RegisterActionTypeCommand, ActionTypeView};
use super::error::RegisterActionTypeError;
use super::ports::ActionRegistrarPort;
use std::sync::Arc;
use tracing::{info, instrument};

/// Use case for registering action types in the Cedar schema
pub struct RegisterActionTypeUseCase<AR>
where
    AR: ActionRegistrarPort,
{
    action_registrar: Arc<AR>,
}

impl<AR> RegisterActionTypeUseCase<AR>
where
    AR: ActionRegistrarPort,
{
    pub fn new(action_registrar: Arc<AR>) -> Self {
        Self { action_registrar }
    }

    #[instrument(skip(self, command), fields(action = %command.action_name))]
    pub async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<ActionTypeView, RegisterActionTypeError> {
        info!("Registering action type: {}", command.action_name);

        // Register action
        self.action_registrar
            .register_action_type(
                command.action_name.clone(),
                command.applies_to_principal.clone(),
                command.applies_to_resource.clone(),
            )
            .await?;

        // Generate fragment for view
        let schema_fragment = format!(
            r#"action "{}" appliesTo {{
    principal: [{}],
    resource: [{}]
}};"#,
            command.action_name,
            command.applies_to_principal,
            command.applies_to_resource
        );

        info!("Action type registered successfully");

        Ok(ActionTypeView {
            action_name: command.action_name,
            schema_fragment,
        })
    }
}
```

---

### Feature 4: `build_schema` (NUEVO)

**Objetivo:** Generar el schema final de Cedar desde todos los tipos registrados

#### Estructura VSA

```
crates/hodei-policies/src/features/build_schema/
├── mod.rs
├── use_case.rs
├── ports.rs
├── dto.rs
├── error.rs
├── di.rs
├── mocks.rs
└── use_case_test.rs
```

#### Contenido de Archivos

**`ports.rs`**

```rust
use async_trait::async_trait;
use super::error::BuildSchemaError;

/// Port for accessing the schema builder to generate final schema
#[async_trait]
pub trait SchemaGeneratorPort: Send + Sync {
    /// Build the final Cedar schema from all registered types
    ///
    /// # Returns
    ///
    /// Complete Cedar schema as string
    async fn build_schema(&self) -> Result<String, BuildSchemaError>;
}
```

**`dto.rs`**

```rust
/// Command to build the schema (no parameters needed)
pub struct BuildSchemaCommand;

/// View of generated schema
pub struct SchemaView {
    pub schema_content: String,
    pub schema_hash: String, // SHA256 for comparison
    pub entity_count: usize,
    pub action_count: usize,
}
```

**`use_case.rs`**

```rust
use super::dto::{BuildSchemaCommand, SchemaView};
use super::error::BuildSchemaError;
use super::ports::SchemaGeneratorPort;
use std::sync::Arc;
use tracing::{info, instrument};
use sha2::{Sha256, Digest};

/// Use case for building the final Cedar schema
pub struct BuildSchemaUseCase<SG>
where
    SG: SchemaGeneratorPort,
{
    schema_generator: Arc<SG>,
}

impl<SG> BuildSchemaUseCase<SG>
where
    SG: SchemaGeneratorPort,
{
    pub fn new(schema_generator: Arc<SG>) -> Self {
        Self { schema_generator }
    }

    #[instrument(skip(self, _command))]
    pub async fn execute(
        &self,
        _command: BuildSchemaCommand,
    ) -> Result<SchemaView, BuildSchemaError> {
        info!("Building Cedar schema");

        // Generate schema
        let schema_content = self.schema_generator.build_schema().await?;

        // Calculate hash for comparison
        let mut hasher = Sha256::new();
        hasher.update(&schema_content);
        let schema_hash = format!("{:x}", hasher.finalize());

        // Parse counts (simplified - could be more sophisticated)
        let entity_count = schema_content.matches("entity ").count();
        let action_count = schema_content.matches("action ").count();

        info!(
            entity_count = entity_count,
            action_count = action_count,
            schema_hash = %schema_hash,
            "Schema built successfully"
        );

        Ok(SchemaView {
            schema_content,
            schema_hash,
            entity_count,
            action_count,
        })
    }
}
```

---

### Feature 5: `persist_schema` (NUEVO)

**Objetivo:** Guardar el schema generado en SurrealDB

#### Estructura VSA

```
crates/hodei-policies/src/features/persist_schema/
├── mod.rs
├── use_case.rs
├── ports.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### Contenido de Archivos

**`ports.rs`**

```rust
use async_trait::async_trait;
use super::dto::PersistedSchemaView;
use super::error::PersistSchemaError;

/// Port for persisting schemas
#[async_trait]
pub trait SchemaStoragePort: Send + Sync {
    /// Save schema to persistent storage
    async fn save_schema(
        &self,
        schema_content: String,
        schema_hash: String,
    ) -> Result<PersistedSchemaView, PersistSchemaError>;

    /// Load latest schema from storage
    async fn load_latest_schema(
        &self,
    ) -> Result<Option<PersistedSchemaView>, PersistSchemaError>;
}
```

**`dto.rs`**

```rust
use chrono::{DateTime, Utc};

/// Command to persist schema
pub struct PersistSchemaCommand {
    pub schema_content: String,
    pub schema_hash: String,
}

/// View of persisted schema
#[derive(Debug, Clone)]
pub struct PersistedSchemaView {
    pub id: String,
    pub schema_content: String,
    pub schema_hash: String,
    pub version: String,
    pub created_at: DateTime<Utc>,
}
```

**`use_case.rs`**

```rust
use super::dto::{PersistSchemaCommand, PersistedSchemaView};
use super::error::PersistSchemaError;
use super::ports::SchemaStoragePort;
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// Use case for persisting Cedar schemas
pub struct PersistSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    schema_storage: Arc<SS>,
}

impl<SS> PersistSchemaUseCase<SS>
where
    SS: SchemaStoragePort,
{
    pub fn new(schema_storage: Arc<SS>) -> Self {
        Self { schema_storage }
    }

    #[instrument(skip(self, command), fields(schema_hash = %command.schema_hash))]
    pub async fn execute(
        &self,
        command: PersistSchemaCommand,
    ) -> Result<PersistedSchemaView, PersistSchemaError> {
        info!("Persisting Cedar schema");

        // Check if schema already exists
        if let Some(existing) = self.schema_storage.load_latest_schema().await? {
            if existing.schema_hash == command.schema_hash {
                info!("Schema unchanged, skipping persistence");
                return Ok(existing);
            }
            warn!("Schema changed, saving new version");
        }

        // Save new schema
        let persisted = self
            .schema_storage
            .save_schema(command.schema_content, command.schema_hash)
            .await?;

        info!(version = %persisted.version, "Schema persisted successfully");

        Ok(persisted)
    }
}
```

---

### Feature 6: `load_schema` (NUEVO)

**Objetivo:** Cargar el schema persistido al inicio

#### Estructura Similar a `persist_schema`

---

## hodei-iam: Features y Estructura Necesaria

### Feature: `register_iam_schema` (NUEVO)

**Objetivo:** Registrar todos los entity types y action types de IAM al arranque

#### Estructura VSA

```
crates/hodei-iam/src/features/register_iam_schema/
├── mod.rs
├── use_case.rs
├── ports.rs
├── dto.rs
├── error.rs
├── di.rs
└── use_case_test.rs
```

#### Contenido de Archivos

**`ports.rs`**

```rust
use async_trait::async_trait;
use super::error::RegisterIamSchemaError;

/// Port for registering entity types (from hodei-policies)
#[async_trait]
pub trait RegisterEntityTypePort: Send + Sync {
    async fn register_entity_type(
        &self,
        entity_type_name: String,
        schema_fragment: String,
    ) -> Result<(), RegisterIamSchemaError>;
}

/// Port for registering action types (from hodei-policies)
#[async_trait]
pub trait RegisterActionTypePort: Send + Sync {
    async fn register_action_type(
        &self,
        action_name: String,
        applies_to_principal: String,
        applies_to_resource: String,
    ) -> Result<(), RegisterIamSchemaError>;
}
```

**`dto.rs`**

```rust
/// Command to register IAM schema (no parameters needed)
pub struct RegisterIamSchemaCommand;

/// View of registered IAM schema
pub struct IamSchemaView {
    pub entity_types_registered: Vec<String>,
    pub action_types_registered: Vec<String>,
}
```

**`use_case.rs`**

```rust
use super::dto::{RegisterIamSchemaCommand, IamSchemaView};
use super::error::RegisterIamSchemaError;
use super::ports::{RegisterEntityTypePort, RegisterActionTypePort};
use crate::internal::domain::{User, Group};
use crate::internal::actions::*;
use std::sync::Arc;
use tracing::{info, instrument};
use kernel::HodeiEntityType;

/// Use case for registering IAM schema at application startup
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
        info!("Registering IAM schema");

        let mut entity_types = Vec::new();
        let mut action_types = Vec::new();

        // Register entity types
        self.register_entity::<User>(&mut entity_types).await?;
        self.register_entity::<Group>(&mut entity_types).await?;

        // Register action types
        self.register_action::<CreateUserAction>(&mut action_types).await?;
        self.register_action::<UpdateUserAction>(&mut action_types).await?;
        self.register_action::<DeleteUserAction>(&mut action_types).await?;
        self.register_action::<CreateGroupAction>(&mut action_types).await?;
        self.register_action::<AddToGroupAction>(&mut action_types).await?;
        // ... más actions

        info!(
            entity_count = entity_types.len(),
            action_count = action_types.len(),
            "IAM schema registered successfully"
        );

        Ok(IamSchemaView {
            entity_types_registered: entity_types,
            action_types_registered: action_types,
        })
    }

    async fn register_entity<T: HodeiEntityType>(
        &self,
        registered: &mut Vec<String>,
    ) -> Result<(), RegisterIamSchemaError> {
        let type_name = format!("{}::{}", T::service_name().as_str(), T::resource_type_name().as_str());
        let schema_fragment = self.generate_entity_fragment::<T>();
        
        self.entity_registrar
            .register_entity_type(type_name.clone(), schema_fragment)
            .await?;
        
        registered.push(type_name);
        Ok(())
    }

    async fn register_action<A: kernel::ActionTrait>(
        &self,
        registered: &mut Vec<String>,
    ) -> Result<(), RegisterIamSchemaError> {
        let action_name = A::name().to_string();
        
        self.action_registrar
            .register_action_type(
                action_name.clone(),
                A::applies_to_principal(),
                A::applies_to_resource(),
            )
            .await?;
        
        registered.push(action_name);
        Ok(())
    }

    fn generate_entity_fragment<T: HodeiEntityType>(&self) -> String {
        // Similar a register_entity_type en hodei-policies
        let mut fragment = format!("entity {} {{\n", T::resource_type_name().as_str());
        for (attr_name, attr_type) in T::attributes_schema() {
            let cedar_type = match attr_type {
                kernel::AttributeType::String => "String",
                kernel::AttributeType::Long => "Long",
                kernel::AttributeType::Boolean => "Bool",
                _ => "String",
            };
            fragment.push_str(&format!("  {}: {},\n", attr_name.as_str(), cedar_type));
        }
        fragment.push_str("}");
        fragment
    }
}
```

---

### Módulo: `internal/actions/` (NUEVO)

**Objetivo:** Definir todas las actions IAM como tipos que implementan `ActionTrait`

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

#### Ejemplo: `create_user_action.rs`

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

## Flujo de Arranque de Aplicación

### Paso 1: Inicialización en `main.rs`

```rust
// src/main.rs

use std::sync::Arc;
use hodei_policies::features::{
    register_entity_type, register_action_type, build_schema, persist_schema
};
use hodei_iam::features::register_iam_schema;
use hodei_policies::infrastructure::schema::SchemaBuilderAdapter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Inicializar infraestructura
    let db = initialize_surrealdb().await?;
    
    // 2. Crear SchemaBuilder (singleton durante arranque)
    let schema_builder = Arc::new(SchemaBuilderAdapter::new());
    
    // 3. Registrar schemas de todos los bounded contexts
    register_all_schemas(schema_builder.clone()).await?;
    
    // 4. Construir schema final
    let schema = build_final_schema(schema_builder.clone()).await?;
    
    // 5. Persistir schema (si cambió)
    persist_schema_if_changed(db.clone(), schema).await?;
    
    // 6. Inicializar aplicación con schema
    let app_state = initialize_app_state(db, schema_builder).await?;
    
    // 7. Arrancar servidor Axum
    start_server(app_state).await?;
    
    Ok(())
}

async fn register_all_schemas(
    schema_builder: Arc<SchemaBuilderAdapter>
) -> Result<(), Box<dyn std::error::Error>> {
    // Registrar IAM
    let iam_registrar = hodei_iam::features::register_iam_schema::di::RegisterIamSchemaUseCaseFactory::build(
        schema_builder.clone(),
        schema_builder.clone(),
    );
    iam_registrar.execute(
        hodei_iam::features::register_iam_schema::dto::RegisterIamSchemaCommand
    ).await?;
    
    // Registrar Organizations
    // let org_registrar = ...
    
    // Registrar Artifacts
    // let artifact_registrar = ...
    
    Ok(())
}

async fn build_final_schema(
    schema_builder: Arc<SchemaBuilderAdapter>
) -> Result<SchemaView, Box<dyn std::error::Error>> {
    let build_use_case = hodei_policies::features::build_schema::di::BuildSchemaUseCaseFactory::build(
        schema_builder
    );
    
    let schema = build_use_case.execute(
        hodei_policies::features::build_schema::dto::BuildSchemaCommand
    ).await?;
    
    Ok(schema)
}

async fn persist_schema_if_changed(
    db: Arc<SurrealDB>,
    schema: SchemaView,
) -> Result<(), Box<dyn std::error::Error>> {
    let persist_use_case = hodei_policies::features::persist_schema::di::PersistSchemaUseCaseFactory::build(
        Arc::new(SurrealSchemaStorageAdapter::new(db))
    );
    
    persist_use_case.execute(
        hodei_policies::features::persist_schema::dto::PersistSchemaCommand {
            schema_content: schema.schema_content,
            schema_hash: schema.schema_hash,
        }
    ).await?;
    
    Ok(())
}
```

---

## Persistencia del Schema

### Tabla SurrealDB

```sql
-- Schema storage table
DEFINE TABLE cedar_schemas SCHEMAFULL;

DEFINE FIELD schema_content ON cedar_schemas TYPE string;
DEFINE FIELD schema_hash ON cedar_schemas TYPE string;
DEFINE FIELD version ON cedar_schemas TYPE string;
DEFINE FIELD created_at ON cedar_schemas TYPE datetime;

-- Index on schema_hash for quick comparison
DEFINE INDEX schema_hash_idx ON cedar_schemas FIELDS schema_hash;
```

### Adaptador de Persistencia

**`crates/hodei-policies/src/infrastructure/surreal/schema_storage_adapter.rs`** (NUEVO)

```rust
use async_trait::async_trait;
use hodei_policies::features::persist_schema::{
    ports::SchemaStoragePort,
    dto::PersistedSchemaView,
    error::PersistSchemaError,
};
use surrealdb::Surreal;
use chrono::Utc;

pub struct SurrealSchemaStorageAdapter {
    db: Arc<Surreal<surrealdb::engine::any::Any>>,
}

impl SurrealSchemaStorageAdapter {
    pub fn new(db: Arc<Surreal<surrealdb::engine::any::Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SchemaStoragePort for SurrealSchemaStorageAdapter {
    async fn save_schema(
        &self,
        schema_content: String,
        schema_hash: String,
    ) -> Result<PersistedSchemaView, PersistSchemaError> {
        let version = format!("v{}", Utc::now().timestamp());
        let created_at = Utc::now();
        
        let record: surrealdb::RecordId = self.db
            .create("cedar_schemas")
            .content(serde_json::json!({
                "schema_content": schema_content,
                "schema_hash": schema_hash,
                "version": version,
                "created_at": created_at,
            }))
            .await
            .map_err(|e| PersistSchemaError::StorageError(e.to_string()))?;
        
        Ok(PersistedSchemaView {
            id: record.to_string(),
            schema_content,
            schema_hash,
            version,
            created_at,
        })
    }
    
    async fn load_latest_schema(
        &self,
    ) -> Result<Option<PersistedSchemaView>, PersistSchemaError> {
        let result: Vec<PersistedSchemaView> = self.db
            .query("SELECT * FROM cedar_schemas ORDER BY created_at DESC LIMIT 1")
            .await
            .map_err(|e| PersistSchemaError::StorageError(e.to_string()))?
            .take(0)
            .map_err(|e| PersistSchemaError::StorageError(e.to_string()))?;
        
        Ok(result.into_iter().next())
    }
}
```

---

## Modificaciones en hodei-iam: `evaluate_iam_policies`

### Cambio Principal: Usar Port en lugar de UseCase

**`crates/hodei-iam/src/features/evaluate_iam_policies/ports.rs`** (MODIFICAR)

```rust
// Agregar nuevo port para evaluación

/// Port for evaluating policies (from hodei-policies)
#[async_trait]
pub trait EvaluatePoliciesPort: Send + Sync {
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError>;
}

// Re-exportar tipos de hodei-policies para conveniencia
pub use hodei_policies::features::evaluate_policies::dto::{
    EvaluatePoliciesCommand,
    EvaluationDecision,
    AuthorizationRequest,
};
pub use hodei_policies::features::evaluate_policies::error::EvaluatePoliciesError;
```

**`crates/hodei-iam/src/features/evaluate_iam_policies/use_case.rs`** (MODIFICAR)

```rust
// Cambiar de:
// use hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase;
// policies_evaluator: EvaluatePoliciesUseCase,

// A:
use super::ports::EvaluatePoliciesPort;

pub struct EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
    EP: EvaluatePoliciesPort, // NUEVO: port en lugar de implementación
{
    policy_finder: Arc<PF>,
    principal_resolver: Arc<PR>,
    resource_resolver: Arc<RR>,
    policies_evaluator: Arc<EP>, // NUEVO: Arc del port
}

impl<PF, PR, RR, EP> EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
    EP: EvaluatePoliciesPort,
{
    pub fn new(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
        policies_evaluator: Arc<EP>, // NUEVO: inyectar port
    ) -> Self {
        Self {
            policy_finder,
            principal_resolver,
            resource_resolver,
            policies_evaluator,
        }
    }
}

// En el método execute:
let evaluation_result = self
    .policies_evaluator
    .evaluate(evaluate_command) // Usar port en lugar de implementación directa
    .await
    .map_err(|e| {
        warn!(error = %e, "Policy evaluation failed");
        AuthorizationError::EvaluationFailed(format!("Cedar evaluation failed: {}", e))
    })?;
```

**`crates/hodei-iam/src/features/evaluate_iam_policies/di.rs`** (MODIFICAR)

```rust
pub struct EvaluateIamPoliciesUseCaseFactory;

impl EvaluateIamPoliciesUseCaseFactory {
    pub fn build<PF, PR, RR, EP>(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
        policies_evaluator: Arc<EP>, // NUEVO: inyectar evaluator
    ) -> EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
    where
        PF: PolicyFinderPort,
        PR: PrincipalResolverPort,
        RR: ResourceResolverPort,
        EP: EvaluatePoliciesPort,
    {
        EvaluateIamPoliciesUseCase::new(
            policy_finder,
            principal_resolver,
            resource_resolver,
            policies_evaluator,
        )
    }
}
```

---

## Plan de Implementación

### Fase 1: hodei-policies - Ports y Features de Schema (ALTA PRIORIDAD)

1. ✅ **Crear `EvaluatePoliciesPort`** en `evaluate_policies/ports.rs`
2. ✅ **Implementar port** en `EvaluatePoliciesUseCase`
3. ✅ **Feature `register_entity_type`** - VSA completa
4. ✅ **Feature `register_action_type`** - VSA completa
5. ✅ **Feature `build_schema`** - VSA completa
6. ✅ **Feature `persist_schema`** - VSA completa
7. ✅ **Feature `load_schema`** - VSA completa
8. ✅ **Adaptador SurrealDB** para `SchemaStoragePort`
9. ✅ **Tests unitarios** para todas las features

### Fase 2: hodei-iam - Actualizar evaluate_iam_policies (ALTA PRIORIDAD)

1. ✅ **Agregar `EvaluatePoliciesPort`** a `ports.rs`
2. ✅ **Modificar `use_case.rs`** para usar port
3. ✅ **Actualizar `di.rs`** para inyectar port
4. ✅ **Actualizar tests** con mocks del port

### Fase 3: hodei-iam - Actions y Schema Registration (MEDIA PRIORIDAD)

1. ✅ **Crear módulo `internal/actions/`**
2. ✅ **Implementar todas las actions** (CreateUser, DeleteUser, etc.)
3. ✅ **Feature `register_iam_schema`** - VSA completa
4. ✅ **Tests unitarios**

### Fase 4: Integration en main.rs (MEDIA PRIORIDAD)

1. ✅ **Flujo de arranque** con registro de schemas
2. ✅ **Inicialización de SchemaBuilder**
3. ✅ **Registro de todos los bounded contexts**
4. ✅ **Persistencia y comparación de schema**

### Fase 5: Otros Bounded Contexts (BAJA PRIORIDAD)

1. ✅ **hodei-organizations**: `register_org_schema`
2. ✅ **hodei-artifacts**: `register_artifact_schema`
3. ✅ **Cada BC** registra sus entities y actions

---

## Checklist de Verificación

### hodei-policies

- [ ] `EvaluatePoliciesPort` definido y implementado
- [ ] Feature `register_entity_type` completa con VSA
- [ ] Feature `register_action_type` completa con VSA
- [ ] Feature `build_schema` completa con VSA
- [ ] Feature `persist_schema` completa con VSA
- [ ] Feature `load_schema` completa con VSA
- [ ] Adaptador SurrealDB para persistencia
- [ ] Todos los tests pasan
- [ ] Código compila sin errores ni warnings
- [ ] API pública exportada en `api.rs`

### hodei-iam

- [ ] `evaluate_iam_policies` usa `EvaluatePoliciesPort`
- [ ] Módulo `internal/actions/` creado
- [ ] Todas las actions IAM implementadas
- [ ] Feature `register_iam_schema` completa con VSA
- [ ] Todos los tests pasan
- [ ] Código compila sin errores ni warnings

### Composition Root (main.rs)

- [ ] Flujo de arranque implementado
- [ ] Schema se construye dinámicamente
- [ ] Schema se persiste en SurrealDB
- [ ] Schema se compara en reinicios
- [ ] Aplicación usa schema correcto

---

## Métricas de Éxito

- ✅ **Cero acoplamiento**: hodei-iam nunca importa implementaciones de hodei-policies
- ✅ **Todo via features**: No hay servicios expuestos directamente
- ✅ **VSA estricto**: Cada operación es una feature completa
- ✅ **ISP respetado**: Ports segregados y específicos
- ✅ **Schema dinámico**: Se construye al arranque desde tipos registrados
- ✅ **Persistencia eficiente**: Solo guarda si el schema cambió
- ✅ **Tests comprehensivos**: 100% coverage de use cases

---

## Conclusión

Esta arquitectura garantiza:

1. **Separación de concerns**: hodei-policies gestiona schema, hodei-iam gestiona identidades
2. **Flexibilidad**: Cualquier BC puede registrar sus tipos
3. **Eficiencia**: Schema se persiste y reutiliza
4. **Mantenibilidad**: Todo sigue VSA estricto
5. **Testabilidad**: Ports permiten mocking completo
6. **Escalabilidad**: Fácil agregar nuevos BCs

El schema de Cedar se convierte en un artefacto de primera clase, gestionado de forma centralizada pero construido de forma descentralizada por todos los bounded contexts.