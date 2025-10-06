# Fase 2 - Tarea 2.1: Dividir create_policy en Features Independientes

**Fecha de inicio:** 2024-01-XX  
**Estado:** 🟡 En Progreso  
**Prioridad:** Alta  
**Estimación:** 8-10 horas

---

## 📋 Resumen Ejecutivo

La feature `create_policy` en `hodei-iam` actualmente contiene **TODOS** los casos de uso CRUD para políticas IAM en un solo módulo. Esto viola:

- ❌ **Principio de Responsabilidad Única (SRP)**: Una feature no debe tener múltiples responsabilidades
- ❌ **Principio de Segregación de Interfaces (ISP)**: El trait `PolicyPersister` tiene todos los métodos CRUD
- ❌ **Vertical Slice Architecture (VSA)**: Cada operación debe ser una "slice" independiente

### Objetivo

Dividir `create_policy` en **5 features independientes**, cada una siguiendo VSA completamente:

1. `create_policy` - Solo CREATE
2. `delete_policy` - Solo DELETE
3. `update_policy` - Solo UPDATE
4. `get_policy` - Solo GET (query individual)
5. `list_policies` - Solo LIST (query con paginación)

---

## 🎯 Análisis del Estado Actual

### Estructura Actual (Monolítica)

```
crates/hodei-iam/src/features/create_policy/
├── use_case.rs              ❌ 5 casos de uso en un solo archivo
├── ports.rs                 ❌ PolicyPersister con 5 métodos
├── dto.rs                   ❌ 5 comandos/queries mezclados
├── error.rs                 ❌ 5 tipos de error mezclados
├── adapter.rs               ⚠️ Implementa los 5 métodos
├── mocks.rs                 ⚠️ Mock implementa los 5 métodos
├── use_case_test.rs         ⚠️ Tests para los 5 casos de uso
└── mod.rs                   ❌ Exporta todo mezclado
```

### Problemas Identificados

#### 1. Trait Monolítico `PolicyPersister`

```rust
#[async_trait]
pub trait PolicyPersister: Send + Sync {
    async fn create_policy(...) -> Result<Policy, CreatePolicyError>;
    async fn delete_policy(...) -> Result<(), DeletePolicyError>;
    async fn update_policy(...) -> Result<Policy, UpdatePolicyError>;
    async fn get_policy(...) -> Result<Policy, GetPolicyError>;
    async fn list_policies(...) -> Result<Vec<Policy>, ListPoliciesError>;
}
```

**Problema:** Un caso de uso que solo necesita DELETE debe implementar o depender de un trait que tiene CREATE, UPDATE, GET, LIST.

**Violación ISP:** Los clientes no deben depender de interfaces que no usan.

#### 2. Archivo `use_case.rs` con 5 Casos de Uso

```rust
pub struct CreatePolicyUseCase<P, V> { ... }
pub struct DeletePolicyUseCase<P> { ... }
pub struct UpdatePolicyUseCase<P, V> { ... }
pub struct GetPolicyUseCase<P> { ... }
pub struct ListPoliciesUseCase<P> { ... }
```

**Problema:** Un solo archivo con múltiples responsabilidades.

**Violación SRP:** Cada caso de uso debería estar en su propio módulo de feature.

#### 3. DTOs Mezclados

```rust
pub struct CreatePolicyCommand { ... }
pub struct DeletePolicyCommand { ... }
pub struct UpdatePolicyCommand { ... }
pub struct GetPolicyQuery { ... }
pub struct ListPoliciesQuery { ... }
pub struct PolicyDto { ... }  // Compartido por todos
```

**Problema:** DTOs de diferentes operaciones en el mismo archivo.

**Mejor práctica VSA:** Cada feature tiene sus propios DTOs.

---

## 🏗️ Estructura Objetivo

### Estructura Después de la Refactorización

```
crates/hodei-iam/src/features/
├── create_policy/           ✅ Solo CREATE
│   ├── mod.rs
│   ├── use_case.rs          # CreatePolicyUseCase
│   ├── ports.rs             # CreatePolicyPort (solo create)
│   ├── dto.rs               # CreatePolicyCommand, PolicyView
│   ├── error.rs             # CreatePolicyError
│   ├── adapter.rs           # Implementación concreta
│   ├── mocks.rs             # Mock para tests
│   ├── use_case_test.rs     # Tests unitarios
│   └── di.rs                # Configuración DI
│
├── delete_policy/           ✅ Solo DELETE
│   ├── mod.rs
│   ├── use_case.rs          # DeletePolicyUseCase
│   ├── ports.rs             # DeletePolicyPort (solo delete)
│   ├── dto.rs               # DeletePolicyCommand
│   ├── error.rs             # DeletePolicyError
│   ├── adapter.rs
│   ├── mocks.rs
│   ├── use_case_test.rs
│   └── di.rs
│
├── update_policy/           ✅ Solo UPDATE
│   ├── mod.rs
│   ├── use_case.rs          # UpdatePolicyUseCase
│   ├── ports.rs             # UpdatePolicyPort (solo update)
│   ├── dto.rs               # UpdatePolicyCommand, PolicyView
│   ├── error.rs             # UpdatePolicyError
│   ├── adapter.rs
│   ├── mocks.rs
│   ├── use_case_test.rs
│   └── di.rs
│
├── get_policy/              ✅ Solo GET
│   ├── mod.rs
│   ├── use_case.rs          # GetPolicyUseCase
│   ├── ports.rs             # GetPolicyPort (solo get)
│   ├── dto.rs               # GetPolicyQuery, PolicyView
│   ├── error.rs             # GetPolicyError
│   ├── adapter.rs
│   ├── mocks.rs
│   ├── use_case_test.rs
│   └── di.rs
│
└── list_policies/           ✅ Solo LIST
    ├── mod.rs
    ├── use_case.rs          # ListPoliciesUseCase
    ├── ports.rs             # ListPoliciesPort (solo list)
    ├── dto.rs               # ListPoliciesQuery, PoliciesListView
    ├── error.rs             # ListPoliciesError
    ├── adapter.rs
    ├── mocks.rs
    ├── use_case_test.rs
    └── di.rs
```

---

## 📝 Plan de Implementación Detallado

### Fase A: Preparación (30 min)

#### A.1 Backup y Documentación
- [x] Documentar estado actual en este archivo
- [ ] Crear backup de `create_policy/` actual
- [ ] Listar todas las dependencias del módulo actual

#### A.2 Análisis de Dependencias
- [ ] Identificar qué código depende de `create_policy`
- [ ] Revisar `lib.rs` para ver exportaciones actuales
- [ ] Verificar tests de integración que usan `create_policy`

---

### Fase B: Feature 1 - `create_policy` (2 horas)

**Objetivo:** Extraer solo la operación CREATE a su propia feature limpia.

#### B.1 Crear Estructura de Directorios (10 min)
```bash
mkdir -p crates/hodei-iam/src/features/create_policy_new/{tests}
```

#### B.2 Crear `dto.rs` (15 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/dto.rs

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Command to create a new IAM policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyCommand {
    /// Unique identifier for the policy
    pub policy_id: String,
    
    /// Cedar policy content (policy text)
    pub policy_content: String,
    
    /// Optional human-readable description
    pub description: Option<String>,
}

/// View of a policy (DTO for responses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyView {
    pub id: Hrn,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

**Checklist:**
- [ ] Crear `dto.rs` con `CreatePolicyCommand`
- [ ] Crear `PolicyView` (DTO de respuesta)
- [ ] Agregar documentación completa
- [ ] Agregar derives necesarios (Debug, Clone, Serialize, Deserialize)

#### B.3 Crear `error.rs` (10 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/error.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreatePolicyError {
    #[error("Policy storage error: {0}")]
    StorageError(String),

    #[error("Invalid policy content: {0}")]
    InvalidPolicyContent(String),

    #[error("Policy validation failed: {0}")]
    ValidationFailed(String),

    #[error("Policy already exists")]
    PolicyAlreadyExists,
    
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),
}
```

**Checklist:**
- [ ] Crear `error.rs` con `CreatePolicyError`
- [ ] Usar `thiserror::Error` para derives
- [ ] Incluir todos los casos de error relevantes
- [ ] Agregar mensajes descriptivos

#### B.4 Crear `ports.rs` (20 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/ports.rs

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use async_trait::async_trait;
use policies::shared::domain::Policy;

/// Port for validating IAM policy content
///
/// Segregated interface - only validation, no persistence
#[async_trait]
pub trait PolicyValidator: Send + Sync {
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError>;
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyValidationError {
    #[error("validation service error: {0}")]
    ServiceError(String),
}

/// Port for creating IAM policies
///
/// SEGREGATED: Only includes create operation (ISP)
#[async_trait]
pub trait CreatePolicyPort: Send + Sync {
    /// Create a new policy
    async fn create(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<Policy, CreatePolicyError>;
}
```

**Checklist:**
- [ ] Crear trait `PolicyValidator` (reutilizable)
- [ ] Crear trait `CreatePolicyPort` (SOLO create)
- [ ] Verificar que NO incluye otros métodos CRUD
- [ ] Documentar el principio ISP aplicado

#### B.5 Crear `use_case.rs` (30 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/use_case.rs

use crate::features::create_policy_new::dto::{CreatePolicyCommand, PolicyView};
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::{CreatePolicyPort, PolicyValidator};
use std::sync::Arc;
use tracing::instrument;

/// Use case for creating IAM policies
///
/// This use case:
/// 1. Validates policy content through PolicyValidator port
/// 2. Persists policy through CreatePolicyPort
/// 3. Returns PolicyView DTO
pub struct CreatePolicyUseCase<P, V>
where
    P: CreatePolicyPort,
    V: PolicyValidator,
{
    policy_port: Arc<P>,
    validator: Arc<V>,
}

impl<P, V> CreatePolicyUseCase<P, V>
where
    P: CreatePolicyPort,
    V: PolicyValidator,
{
    pub fn new(policy_port: Arc<P>, validator: Arc<V>) -> Self {
        Self {
            policy_port,
            validator,
        }
    }

    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<PolicyView, CreatePolicyError> {
        // 1. Validate policy content
        let validation_result = self
            .validator
            .validate_policy(&command.policy_content)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed(e.to_string()))?;

        if !validation_result.is_valid {
            let error_messages: Vec<String> = validation_result
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect();
            return Err(CreatePolicyError::InvalidPolicyContent(
                error_messages.join("; "),
            ));
        }

        // 2. Create policy through port
        let policy = self.policy_port.create(command).await?;

        // 3. Return DTO
        Ok(PolicyView {
            id: policy.id,
            content: policy.content,
            description: policy.description,
            created_at: policy.created_at,
            updated_at: policy.updated_at,
        })
    }
}
```

**Checklist:**
- [ ] Crear `CreatePolicyUseCase` struct
- [ ] Implementar constructor `new()`
- [ ] Implementar `execute()` con lógica de negocio
- [ ] Agregar instrumentación con `tracing`
- [ ] Documentar el flujo del caso de uso

#### B.6 Crear `mocks.rs` (20 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/mocks.rs

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::{
    CreatePolicyPort, PolicyValidationError, PolicyValidator, ValidationResult,
};
use async_trait::async_trait;
use policies::shared::domain::Policy;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct MockPolicyValidator {
    pub should_fail: bool,
    pub validation_errors: Vec<String>,
}

#[async_trait]
impl PolicyValidator for MockPolicyValidator {
    async fn validate_policy(
        &self,
        _policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError> {
        if self.should_fail {
            return Err(PolicyValidationError::ServiceError(
                "Mock validation service error".to_string(),
            ));
        }

        let is_valid = self.validation_errors.is_empty();
        let errors = self
            .validation_errors
            .iter()
            .map(|msg| crate::features::create_policy_new::ports::ValidationError {
                message: msg.clone(),
                line: None,
                column: None,
            })
            .collect();

        Ok(ValidationResult {
            is_valid,
            errors,
            warnings: vec![],
        })
    }
}

#[derive(Debug, Default)]
pub struct MockCreatePolicyPort {
    pub should_fail: bool,
    pub created_policies: Arc<Mutex<Vec<Policy>>>,
}

#[async_trait]
impl CreatePolicyPort for MockCreatePolicyPort {
    async fn create(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<Policy, CreatePolicyError> {
        if self.should_fail {
            return Err(CreatePolicyError::StorageError(
                "Mock storage error".to_string(),
            ));
        }

        let policy = Policy {
            id: kernel::Hrn::parse(&format!("hrn:hodei:iam::test:policy/{}", command.policy_id))
                .map_err(|e| CreatePolicyError::InvalidHrn(e.to_string()))?,
            content: command.policy_content,
            description: command.description,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        self.created_policies.lock().unwrap().push(policy.clone());
        Ok(policy)
    }
}
```

**Checklist:**
- [ ] Crear `MockPolicyValidator`
- [ ] Crear `MockCreatePolicyPort`
- [ ] Permitir configurar éxito/fallo en tests
- [ ] Registrar operaciones para verificación en tests

#### B.7 Crear `use_case_test.rs` (30 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/use_case_test.rs

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::mocks::{MockCreatePolicyPort, MockPolicyValidator};
use crate::features::create_policy_new::use_case::CreatePolicyUseCase;
use std::sync::Arc;

#[tokio::test]
async fn test_create_policy_success() {
    // Arrange
    let validator = Arc::new(MockPolicyValidator::default());
    let port = Arc::new(MockCreatePolicyPort::default());
    let use_case = CreatePolicyUseCase::new(port.clone(), validator);

    let command = CreatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: Some("Test policy".to_string()),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.content, "permit(principal, action, resource);");
    assert_eq!(view.description, Some("Test policy".to_string()));

    // Verify policy was created
    let created = port.created_policies.lock().unwrap();
    assert_eq!(created.len(), 1);
}

#[tokio::test]
async fn test_create_policy_validation_fails() {
    // Arrange
    let mut validator = MockPolicyValidator::default();
    validator.validation_errors = vec!["Invalid syntax".to_string()];
    let validator = Arc::new(validator);
    
    let port = Arc::new(MockCreatePolicyPort::default());
    let use_case = CreatePolicyUseCase::new(port.clone(), validator);

    let command = CreatePolicyCommand {
        policy_id: "invalid-policy".to_string(),
        policy_content: "invalid policy content".to_string(),
        description: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    matches!(result.unwrap_err(), CreatePolicyError::InvalidPolicyContent(_));

    // Verify policy was NOT created
    let created = port.created_policies.lock().unwrap();
    assert_eq!(created.len(), 0);
}

#[tokio::test]
async fn test_create_policy_storage_error() {
    // Arrange
    let validator = Arc::new(MockPolicyValidator::default());
    let mut port = MockCreatePolicyPort::default();
    port.should_fail = true;
    let port = Arc::new(port);
    
    let use_case = CreatePolicyUseCase::new(port, validator);

    let command = CreatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: None,
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    matches!(result.unwrap_err(), CreatePolicyError::StorageError(_));
}
```

**Checklist:**
- [ ] Test: creación exitosa
- [ ] Test: validación falla
- [ ] Test: error de almacenamiento
- [ ] Test: política ya existe
- [ ] Verificar que mocks funcionan correctamente
- [ ] Todos los tests pasan

#### B.8 Crear `adapter.rs` (15 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/adapter.rs

// Placeholder - implementación real dependerá de la infraestructura
// Por ahora, stub para compilación

use crate::features::create_policy_new::dto::CreatePolicyCommand;
use crate::features::create_policy_new::error::CreatePolicyError;
use crate::features::create_policy_new::ports::CreatePolicyPort;
use async_trait::async_trait;
use policies::shared::domain::Policy;

pub struct StubCreatePolicyAdapter;

#[async_trait]
impl CreatePolicyPort for StubCreatePolicyAdapter {
    async fn create(
        &self,
        _command: CreatePolicyCommand,
    ) -> Result<Policy, CreatePolicyError> {
        // TODO: Implement with real infrastructure
        unimplemented!("CreatePolicyPort adapter not yet implemented")
    }
}
```

**Checklist:**
- [ ] Crear stub adapter
- [ ] Documentar que es temporal
- [ ] Será implementado cuando se conecte con infraestructura real

#### B.9 Crear `di.rs` (10 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/di.rs

// Placeholder for dependency injection configuration
// Will be implemented when connecting to infrastructure layer

use crate::features::create_policy_new::use_case::CreatePolicyUseCase;
use crate::features::create_policy_new::adapter::StubCreatePolicyAdapter;
use crate::features::create_policy_new::ports::{CreatePolicyPort, PolicyValidator};
use std::sync::Arc;

/// Create use case with stub dependencies (for development)
pub fn create_policy_use_case_stub() -> CreatePolicyUseCase<
    StubCreatePolicyAdapter,
    impl PolicyValidator
> {
    // TODO: Replace with real implementations
    unimplemented!("DI configuration not yet implemented")
}
```

**Checklist:**
- [ ] Crear estructura básica de DI
- [ ] Documentar que será completado en fase posterior

#### B.10 Crear `mod.rs` (10 min)
```rust
// crates/hodei-iam/src/features/create_policy_new/mod.rs

pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

// Adapters and DI (internal, not public API)
pub(crate) mod adapter;
pub(crate) mod di;

// Test utilities
#[cfg(test)]
pub(crate) mod mocks;
#[cfg(test)]
mod use_case_test;

// Public API exports
pub use dto::{CreatePolicyCommand, PolicyView};
pub use error::CreatePolicyError;
pub use use_case::CreatePolicyUseCase;
```

**Checklist:**
- [ ] Exportar solo API pública (DTO, error, use case)
- [ ] Mantener adapter/di/mocks como internos
- [ ] Verificar que compila

---

### Fase C: Features 2-5 - DELETE, UPDATE, GET, LIST (4 horas)

**Para cada feature restante, seguir el mismo patrón:**

1. Crear estructura de directorios
2. Crear `dto.rs` (comando/query específico)
3. Crear `error.rs` (error específico)
4. Crear `ports.rs` (port segregado - SOLO una operación)
5. Crear `use_case.rs` (lógica de negocio)
6. Crear `mocks.rs` (mocks para tests)
7. Crear `use_case_test.rs` (tests unitarios)
8. Crear `adapter.rs` (stub)
9. Crear `di.rs` (configuración)
10. Crear `mod.rs` (exports)

**Notas específicas por feature:**

#### C.1 `delete_policy`
- Port: `DeletePolicyPort` con solo método `delete()`
- No necesita `PolicyValidator` (no hay contenido a validar)
- Tests: éxito, política no encontrada, error de storage

#### C.2 `update_policy`
- Port: `UpdatePolicyPort` con solo método `update()`
- SÍ necesita `PolicyValidator` (reutilizar del kernel o shared)
- DTO: `UpdatePolicyCommand` con campos opcionales
- Tests: éxito, validación falla, política no encontrada

#### C.3 `get_policy`
- Port: `GetPolicyPort` con solo método `get()`
- No necesita validación (es una query)
- DTO: `GetPolicyQuery` con policy_id
- Tests: éxito, política no encontrada

#### C.4 `list_policies`
- Port: `ListPoliciesPort` con solo método `list()`
- DTO: `ListPoliciesQuery` con paginación (limit, offset)
- Return: `PoliciesListView` con lista de `PolicyView`
- Tests: éxito con paginación, lista vacía

---

### Fase D: Integración y Limpieza (1.5 horas)

#### D.1 Actualizar `features/mod.rs` (10 min)
```rust
// crates/hodei-iam/src/features/mod.rs

pub mod add_user_to_group;
pub mod create_group;
pub mod create_user;
pub mod evaluate_iam_policies;
pub mod get_effective_policies_for_principal;

// Nuevas features segregadas
pub mod create_policy;
pub mod delete_policy;
pub mod update_policy;
pub mod get_policy;
pub mod list_policies;

// Feature antigua (deprecada)
#[deprecated(
    since = "0.2.0",
    note = "Monolithic feature - use segregated features instead"
)]
pub mod create_policy_old;
```

**Checklist:**
- [ ] Agregar módulos de las 5 nuevas features
- [ ] Renombrar `create_policy` antigua a `create_policy_old`
- [ ] Marcar como deprecated
- [ ] Actualizar exports

#### D.2 Actualizar `lib.rs` (20 min)
```rust
// crates/hodei-iam/src/lib.rs

// Policy Management Features (Segregated)
pub use features::create_policy::{
    CreatePolicyCommand, CreatePolicyError, CreatePolicyUseCase, PolicyView,
};
pub use features::delete_policy::{
    DeletePolicyCommand, DeletePolicyError, DeletePolicyUseCase,
};
pub use features::update_policy::{
    UpdatePolicyCommand, UpdatePolicyError, UpdatePolicyUseCase,
};
pub use features::get_policy::{
    GetPolicyError, GetPolicyQuery, GetPolicyUseCase,
};
pub use features::list_policies::{
    ListPoliciesError, ListPoliciesQuery, ListPoliciesUseCase, PoliciesListView,
};
```

**Checklist:**
- [ ] Exportar las 5 nuevas features
- [ ] Mantener exports de features existentes
- [ ] Actualizar documentación del módulo
- [ ] Verificar que compila

#### D.3 Renombrar Directorio Antiguo (5 min)
```bash
mv crates/hodei-iam/src/features/create_policy \
   crates/hodei-iam/src/features/create_policy_old
```

**Checklist:**
- [ ] Renombrar directorio antiguo
- [ ] Actualizar referencias en `mod.rs`
- [ ] Marcar como deprecated

#### D.4 Verificación de Compilación (15 min)
```bash
# Compilar el crate
cargo check -p hodei-iam --all-features

# Ejecutar tests
cargo test -p hodei-iam --lib

# Verificar clippy
cargo clippy -p hodei-iam --all-features
```

**Checklist:**
- [ ] `cargo check` exitoso
- [ ] Tests unitarios pasan
- [ ] Warnings de clippy resueltos o documentados

#### D.5 Actualizar Documentación (30 min)
- [ ] Actualizar `REFACTOR_PROGRESS.md`
- [ ] Marcar Tarea 2.1 como completada
- [ ] Documentar decisiones de diseño
- [ ] Crear guía de migración para consumidores

---

### Fase E: Tests de Integración (1 hora)

#### E.1 Crear Tests de Integración por Feature
```bash
mkdir -p crates/hodei-iam/tests/policies/
touch crates/hodei-iam/tests/policies/{create,delete,update,get,list}_policy_test.rs
```

**Checklist:**
- [ ] Test de integración para create_policy
- [ ] Test de integración para delete_policy
- [ ] Test de integración para update_policy
- [ ] Test de integración para get_policy
- [ ] Test de integración para list_policies

#### E.2 Ejecutar Suite Completa de Tests
```bash
cargo test -p hodei-iam
```

**Checklist:**
- [ ] Todos los tests unitarios pasan
- [ ] Todos los tests de integración pasan
- [ ] Cobertura de tests > 80% por feature

---

## ✅ Criterios de Aceptación

### Funcionales
- [ ] 5 features independientes creadas
- [ ] Cada feature sigue estructura VSA completa
- [ ] Cada feature tiene su propio port segregado (ISP)
- [ ] Todos los tests unitarios pasan
- [ ] Todos los tests de integración pasan

### Arquitectónicos
- [ ] Principio ISP aplicado (un método por trait)
- [ ] Principio SRP aplicado (una responsabilidad por feature)
- [ ] VSA completa en cada feature
- [ ] Sin dependencias cruzadas entre features de políticas

### Calidad de Código
- [ ] Código compila sin errores
- [ ] Sin warnings de clippy (o documentados si inevitables)
- [ ] Documentación completa en todos los módulos públicos
- [ ] Tests con cobertura > 80%

### Documentación
- [ ] `REFACTOR_PROGRESS.md` actualizado
- [ ] Esta tarea marcada como completada
- [ ] Guía de migración creada
- [ ] Ejemplos de uso en `lib.rs`

---

## 📊 Métricas de Éxito

| Métrica | Antes | Objetivo | Estado |
|---------|-------|----------|--------|
| Features de políticas | 1 (monolítica) | 5 (segregadas) | ⚪ |
| Métodos en PolicyPersister | 5 | 1 por trait | ⚪ |
| Tests unitarios | ~10 | ~25 (5 por feature) | ⚪ |
| Tests de integración | 0 | 5 | ⚪ |
| Warnings clippy | 10 | < 5 | ⚪ |
| Líneas por use_case.rs | ~200 | ~50 | ⚪ |

---

## 🚨 Riesgos y Mitigaciones

### Riesgo 1: Breaking Changes para Consumidores
**Mitigación:**
- Mantener feature antigua como `create_policy_old` deprecated
- Crear guía de migración clara
- Deprecar gradualmente (no eliminar inmediatamente)

### Riesgo 2: Código Duplicado entre Features
**Mitigación:**
- `PolicyView` DTO puede ser compartido (crear en `internal/domain/`)
- `PolicyValidator` trait puede ser compartido (mover a `internal/application/ports/`)
- Solo duplicar lo verdaderamente específico de cada feature

### Riesgo 3: Tests Pueden Fallar Durante Refactorización
**Mitigación:**
- Trabajar feature por feature
- Verificar compilación después de cada feature
- Mantener feature antigua funcional hasta que todas las nuevas estén listas

---

## 📚 Referencias

### Modelos a Seguir
1. **`hodei-organizations/features/create_account`** - Excelente ejemplo de VSA
2. **`hodei-organizations/features/get_effective_scps`** - Query bien implementada
3. **Kernel `AuthContextProvider`** - Trait bien segregado

### Principios Aplicados
- **SOLID - ISP**: Interface Segregation Principle
- **SOLID - SRP**: Single Responsibility Principle
- **VSA**: Vertical Slice Architecture
- **Clean Architecture**: Separación de capas

---

## 🎯 Siguiente Paso Inmediato

**Comenzar con Fase B: Feature 1 - `create_policy`**

Comando para empezar:
```bash
mkdir -p crates/hodei-iam/src/features/create_policy_new
touch crates/hodei-iam/src/features/create_policy_new/{mod,dto,error,ports,use_case,adapter,mocks,use_case_test,di}.rs
```

---

**Última actualización:** 2024-01-XX  
**Estado:** 📋 PLANIFICADO - Listo para comenzar implementación