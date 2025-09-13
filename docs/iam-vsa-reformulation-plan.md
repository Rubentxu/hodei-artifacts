# Plan de Reformulación del Crate IAM según Vertical Slice Architecture (VSA)

## 📋 Resumen Ejecutivo

Este documento detalla el plan completo para reformular el crate IAM de Hodei Artifacts según los principios de Vertical Slice Architecture (VSA), abordando los problemas identificados y estableciendo una arquitectura limpia, mantenible y testeable.

## 🔍 Análisis de Problemas Actuales

### Problemas Identificados según VSA:

1. **Acoplamiento en `application/ports.rs`**: 
   - Se comparten `PolicyFilter` y `PolicyList` entre todas las features
   - Violación del principio de segregación de interfaces

2. **Violación de segregación de interfaces**:
   - `list_policies/ports.rs` importa desde `application/ports.rs`
   - Cada feature debería definir sus propios DTOs y filtros

3. **Estructura de errores no específica por feature**:
   - Uso de `IamError` genérico desde `infrastructure/errors.rs`
   - Falta de errores específicos con `thiserror` por feature

4. **Tests incompletos**:
   - Tests en `api.rs` están comentados o incompletos
   - Falta implementar tests unitarios completos con mocks

5. **Problemas de nomenclatura y estructura**:
   - Algunas features no siguen la convención de nombres VSA
   - Falta de `error.rs` específicos por feature

## 🎯 Objetivos de la Reformulación

1. **Cumplir estrictamente con VSA**: Cada feature debe ser independiente
2. **Eliminar acoplamiento**: No compartir interfaces entre features
3. **Implementar errores específicos**: Cada feature con su propio `error.rs`
4. **Tests completos**: Cobertura unitaria completa con mocks
5. **Compilación limpia**: Sin errores ni warnings
6. **Documentación**: Arquitectura final documentada

## 📦 Estructura de Features del IAM

### Features Actuales:
- `create_policy` - Crear políticas Cedar
- `delete_policy` - Eliminar políticas
- `get_policy` - Obtener política específica
- `list_policies` - Listar políticas con filtros
- `update_policy` - Actualizar políticas
- `validate_policy` - Validar sintaxis y semántica

### Features Comentadas (requieren atención):
- `analyze_policy_coverage` - Análisis de cobertura
- `detect_policy_conflicts` - Detección de conflictos

## 🔧 Plan de Implementación Detallado

### Fase 1: Estructura de Errores Específicos

#### 1.1 Crear `error.rs` para cada feature:

**create_policy/error.rs:**
```rust
use thiserror::Error;
use shared::hrn::PolicyId;

#[derive(Debug, Error)]
pub enum CreatePolicyError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Policy validation failed: {errors:?}")]
    ValidationFailed { errors: Vec<String> },
    
    #[error("Policy already exists: {0}")]
    PolicyAlreadyExists(PolicyId),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Event publishing failed: {0}")]
    EventPublishingFailed(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<CreatePolicyError> for crate::infrastructure::errors::IamError {
    fn from(error: CreatePolicyError) -> Self {
        match error {
            CreatePolicyError::InvalidInput(msg) => Self::InvalidInput(msg),
            CreatePolicyError::ValidationFailed { errors } => Self::PolicyValidationFailed { errors },
            CreatePolicyError::PolicyAlreadyExists(id) => Self::PolicyAlreadyExists(id),
            CreatePolicyError::DatabaseError(msg) => Self::DatabaseError(msg),
            CreatePolicyError::EventPublishingFailed(msg) => Self::InternalError(msg),
            CreatePolicyError::InternalError(msg) => Self::InternalError(msg),
        }
    }
}
```

**list_policies/error.rs:**
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ListPoliciesError {
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<ListPoliciesError> for crate::infrastructure::errors::IamError {
    fn from(error: ListPoliciesError) -> Self {
        match error {
            ListPoliciesError::InvalidFilter(msg) => Self::InvalidInput(msg),
            ListPoliciesError::DatabaseError(msg) => Self::DatabaseError(msg),
            ListPoliciesError::InternalError(msg) => Self::InternalError(msg),
        }
    }
}
```

### Fase 2: Refactorización de Ports y DTOs

#### 2.1 Eliminar dependencia de `application/ports.rs`:

**list_policies/ports.rs** (nueva versión):
```rust
use crate::features::list_policies::dto::{ListPoliciesQuery, PolicyListResponse};
use crate::features::list_policies::error::ListPoliciesError;
use async_trait::async_trait;

#[async_trait]
pub trait PolicyLister: Send + Sync {
    async fn list(&self, query: ListPoliciesQuery) -> Result<PolicyListResponse, ListPoliciesError>;
    async fn count(&self, query: ListPoliciesQuery) -> Result<u64, ListPoliciesError>;
}
```

**list_policies/dto.rs** (con filtros específicos):
```rust
#[derive(Debug, Clone, Default)]
pub struct ListPoliciesQuery {
    pub name: Option<String>,
    pub name_contains: Option<String>,
    pub status: Option<PolicyStatus>,
    pub tags: Vec<String>,
    pub created_by: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub sort_by: Option<PolicySortBy>,
    pub sort_order: Option<SortOrder>,
}

#[derive(Debug, Clone)]
pub struct PolicyListResponse {
    pub policies: Vec<Policy>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub has_more: bool,
}
```

### Fase 3: Actualización de Use Cases

#### 3.1 Modificar `create_policy/use_case.rs`:
```rust
use crate::features::create_policy::error::CreatePolicyError;

impl CreatePolicyUseCase {
    pub async fn execute(&self, command: CreatePolicyCommand) -> Result<CreatePolicyResponse, CreatePolicyError> {
        // Cambiar todos los IamError por CreatePolicyError específicos
    }
}
```

### Fase 4: Implementación de Tests Completos

#### 4.1 Tests para `api.rs` con tracing asserts:

**create_policy/api_test.rs:**
```rust
use tracing_test::traced_test;
use tracing::Level;

#[tokio::test]
#[traced_test]
async fn test_create_policy_api_success() {
    // Setup mocks
    let mock_creator = Arc::new(MockPolicyCreator::new());
    let mock_validator = Arc::new(MockPolicyValidator::new());
    let mock_publisher = Arc::new(MockPolicyEventPublisher::new());
    
    let use_case = Arc::new(CreatePolicyUseCase::new(
        mock_creator,
        mock_validator,
        mock_publisher,
    ));
    
    let api = CreatePolicyApi::new(use_case);
    
    let command = CreatePolicyCommand::new(
        "Test Policy".to_string(),
        "permit(principal, action, resource);".to_string(),
        "user_123".to_string(),
    );
    
    let result = api.create_policy(command).await;
    
    assert!(result.is_ok());
    
    // Verificar logs con tracing
    assert!(logs_contain("Creating policy: Test Policy"));
    assert!(logs_contain("Policy created successfully"));
    
    // Verificar spans
    let spans = get_spans_at_level(Level::INFO);
    assert!(!spans.is_empty());
    assert!(spans.iter().any(|span| span.name == "create_policy"));
}
```

### Fase 5: Mocks Completos en Adapters

#### 5.1 Implementar mocks completos en `adapter.rs`:

**create_policy/adapter.rs:**
```rust
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::{Arc, Mutex};
    
    pub struct MockPolicyCreator {
        policies: Arc<Mutex<Vec<Policy>>>,
        should_fail: bool,
        existing_ids: Vec<String>,
    }
    
    impl MockPolicyCreator {
        pub fn new() -> Self {
            Self {
                policies: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
                existing_ids: Vec::new(),
            }
        }
        
        pub fn with_failure() -> Self {
            Self {
                policies: Arc::new(Mutex::new(Vec::new())),
                should_fail: true,
                existing_ids: Vec::new(),
            }
        }
        
        pub fn with_existing_policy(mut self, id: String) -> Self {
            self.existing_ids.push(id);
            self
        }
        
        pub fn get_saved_policies(&self) -> Vec<Policy> {
            self.policies.lock().unwrap().clone()
        }
    }
    
    #[async_trait]
    impl PolicyCreator for MockPolicyCreator {
        async fn create(&self, policy: Policy) -> Result<Policy, CreatePolicyError> {
            if self.should_fail {
                return Err(CreatePolicyError::DatabaseError("Mock database error".to_string()));
            }
            
            let mut policies = self.policies.lock().unwrap();
            policies.push(policy.clone());
            Ok(policy)
        }
        
        async fn exists(&self, id: &PolicyId) -> Result<bool, CreatePolicyError> {
            if self.should_fail {
                return Err(CreatePolicyError::DatabaseError("Mock exists check error".to_string()));
            }
            Ok(self.existing_ids.contains(&id.0.to_string()))
        }
    }
}
```

## 📊 Criterios de Éxito

### Compilación:
- ✅ `cargo check` sin errores
- ✅ `cargo clippy` sin warnings
- ✅ `cargo build` exitoso

### Testing:
- ✅ Tests unitarios: 100% cobertura de use_case.rs y api.rs
- ✅ Tests con mocks completos
- ✅ Asserts de tracing funcionando
- ✅ Tests de integración con testcontainers

### Arquitectura:
- ✅ Cada feature con sus propios ports segregados
- ✅ Errores específicos por feature con `thiserror`
- ✅ Sin acoplamiento entre features
- ✅ Tests unitarios completos con mocks

## 🚀 Próximos Pasos

1. **Implementar el plan**: Cambiar al modo Code para ejecutar las reformulaciones
2. **Verificar compilación**: Asegurar que todo compile sin errores
3. **Ejecutar tests**: Validar que todos los tests pasen
4. **Documentar**: Actualizar documentación de la arquitectura

## 💡 Recomendaciones Adicionales

1. **Uso de `cargo-nextest`**: Para ejecución rápida de tests en desarrollo
2. **Integración con CI**: Automatizar verificación de VSA en pipeline
3. **Monitoreo de deuda técnica**: Revisar periódicamente nuevos acoplamientos
4. **Formación del equipo**: Documentar y compartir principios VSA

## 📋 Checklist Final

- [ ] Todos los features tienen `error.rs` específico
- [ ] Todos los features tienen ports segregados
- [ ] Tests unitarios completos para use_case.rs
- [ ] Tests unitarios completos para api.rs con tracing
- [ ] Mocks completos en adapter.rs
- [ ] Compilación sin errores ni warnings
- [ ] Tests de integración con testcontainers
- [ ] Documentación actualizada

---

**Nota**: Este plan debe ser ejecutado en modo Code para implementar los cambios necesarios en los archivos Rust.