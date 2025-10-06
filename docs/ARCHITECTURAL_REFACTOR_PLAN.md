# Plan de Refactorización Arquitectónica - Hodei Artifacts

## 📋 Resumen Ejecutivo

Este documento detalla el plan de refactorización para corregir las **5 violaciones arquitectónicas críticas** identificadas en el análisis de código, siguiendo estrictamente los principios de:
- **Clean Architecture**
- **Vertical Slice Architecture (VSA)**
- **Domain-Driven Design (DDD)**
- **SOLID Principles**

---

## 🎯 Problemas Identificados

### 1. Violación Estructural: Módulos `shared` Internos (CRÍTICO)
**Crates afectados:** `hodei-iam`, `hodei-organizations`

**Problema:**
```
crates/hodei-iam/src/shared/
├── domain/entities.rs          ❌ User, Group
├── application/ports/mod.rs    ❌ UserRepository, GroupRepository
└── infrastructure/             ❌ InMemoryUserRepository

crates/hodei-organizations/src/shared/
├── domain/account.rs           ❌ Account
└── application/ports/          ❌ AccountRepository
```

**Impacto:**
- Rompe el encapsulamiento del Bounded Context
- Crea ambigüedad sobre si `User` es interno o compartido
- Viola la Regla #1 (Bounded Contexts como crates independientes)
- Viola la Regla #3 (Kernel compartido centralizado)

**Solución:**
```
crates/hodei-iam/src/
├── features/                   ✅ API pública
│   ├── create_user/
│   ├── add_user_to_group/
│   └── ...
├── internal/                   ✅ Módulo PRIVADO (no pub mod)
│   ├── domain/
│   │   ├── user.rs
│   │   └── group.rs
│   ├── ports/
│   │   ├── user_repository.rs
│   │   └── group_repository.rs
│   └── infrastructure/
│       └── ...
└── lib.rs                      ✅ Solo exporta features

crates/kernel/                  ✅ Shared Kernel centralizado
├── domain/
│   ├── hrn.rs                  ✅ Tipos verdaderamente compartidos
│   ├── aggregate.rs
│   └── events.rs
└── application/
    └── ports/
        └── auth.rs             ✅ Traits transversales
```

---

### 2. Fuga de Implementación: Exposición Pública de Internals (CRÍTICO)
**Crate afectado:** `hodei-iam`

**Problema:**
```rust
// crates/hodei-iam/src/lib.rs
pub mod infrastructure { ... }  ❌ Expone InMemoryUserRepository
pub mod ports { ... }           ❌ Expone UserRepository trait
pub mod shared { ... }          ❌ Expone entidades de dominio
```

**Impacto:**
- Permite a consumidores externos depender de detalles internos
- Crea acoplamiento fuerte (Connascence de Ubicación y Nombre)
- Anula los beneficios de la Arquitectura Hexagonal
- Viola la Regla #2 (Exposición Mínima)

**Solución:**
```rust
// crates/hodei-iam/src/lib.rs
mod internal;                   ✅ Módulo privado

pub mod features;               ✅ Solo features públicas

// Re-exportar ÚNICAMENTE casos de uso y DTOs
pub use features::create_user::{CreateUserUseCase, CreateUserCommand};
pub use features::add_user_to_group::{AddUserToGroupUseCase, AddUserToGroupCommand};
pub use features::get_effective_policies::{GetEffectivePoliciesUseCase, GetEffectivePoliciesQuery};

// NO exportar:
// - Entidades de dominio (User, Group)
// - Puertos de repositorio (UserRepository)
// - Implementaciones de infraestructura (InMemoryUserRepository)
```

---

### 3. Feature Slice Monolítica: CRUD en un Solo Módulo (ALTO)
**Crate afectado:** `hodei-iam`

**Problema:**
```
crates/hodei-iam/src/features/create_policy/
├── use_case.rs                 ❌ Contiene Create, Delete, Update, Get, List
└── ports.rs                    ❌ PolicyPersister con todos los métodos
```

**Impacto:**
- Viola el Principio de Responsabilidad Única (SRP)
- Viola el Principio de Segregación de Interfaces (ISP)
- Aumenta complejidad del módulo
- Un caso de uso `ListPolicies` no necesita `create_policy()`

**Solución:**
```
crates/hodei-iam/src/features/
├── create_policy/
│   ├── mod.rs
│   ├── use_case.rs             ✅ Solo CreatePolicyUseCase
│   ├── ports.rs                ✅ Solo CreatePolicyPort
│   ├── dto.rs                  ✅ CreatePolicyCommand
│   ├── error.rs                ✅ CreatePolicyError
│   ├── adapter.rs
│   ├── use_case_test.rs
│   └── di.rs
├── delete_policy/
│   ├── mod.rs
│   ├── use_case.rs             ✅ Solo DeletePolicyUseCase
│   ├── ports.rs                ✅ Solo DeletePolicyPort
│   ├── dto.rs                  ✅ DeletePolicyCommand
│   ├── error.rs                ✅ DeletePolicyError
│   ├── adapter.rs
│   ├── use_case_test.rs
│   └── di.rs
├── update_policy/...
├── get_policy/...
└── list_policies/...
```

**Ejemplo de puerto segregado:**
```rust
// crates/hodei-iam/src/features/delete_policy/ports.rs
pub trait DeletePolicyPort: Send + Sync {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError>;
}

// crates/hodei-iam/src/features/list_policies/ports.rs
pub trait ListPoliciesPort: Send + Sync {
    async fn list(&self) -> Result<Vec<PolicyView>, ListPoliciesError>;
}
```

---

### 4. Acoplamiento Invertido: Infraestructura → Aplicación (MEDIO)
**Crate afectado:** `hodei-organizations`

**Problema:**
```rust
// crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs
impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, hrn: &Hrn) -> Result<PolicySet, Error> {
        // ❌ Infraestructura invocando caso de uso de su propia aplicación
        let use_case = get_effective_scps_use_case(...);
        use_case.execute(...).await
    }
}
```

**Impacto:**
- Invierte la dirección de dependencias (Infraestructura → Aplicación)
- Crea ciclo de dependencias conceptual
- Dificulta razonamiento sobre flujo de control
- Viola Dependency Inversion Principle (DIP)

**Solución:**
```rust
// La implementación del adaptador debe contener la lógica de negocio directamente
impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, hrn: &Hrn) -> Result<PolicySet, Error> {
        // ✅ Usar repositorios inyectados, no casos de uso
        
        // 1. Determinar tipo de recurso (Account o OU)
        let resource_type = self.determine_resource_type(hrn).await?;
        
        // 2. Cargar entidad usando repositorios
        let entity = match resource_type {
            ResourceType::Account => self.account_repo.find_by_hrn(hrn).await?,
            ResourceType::OU => self.ou_repo.find_by_hrn(hrn).await?,
        };
        
        // 3. Recorrer jerarquía de OUs hacia raíz
        let mut scp_hrns = Vec::new();
        let mut current_ou = entity.parent_ou_id();
        
        while let Some(ou_id) = current_ou {
            let ou = self.ou_repo.find_by_id(ou_id).await?;
            scp_hrns.extend(ou.attached_scp_hrns());
            current_ou = ou.parent_ou_id();
        }
        
        // 4. Cargar contenido de SCPs
        let scps = self.scp_repo.find_by_hrns(&scp_hrns).await?;
        
        // 5. Construir PolicySet de Cedar
        let policy_set = self.build_policy_set(scps)?;
        
        Ok(policy_set)
    }
}
```

---

### 5. Errores Genéricos: `anyhow::Error` en Casos de Uso (MEDIO)
**Crate afectado:** `hodei-iam`

**Problema:**
```rust
// crates/hodei-iam/src/features/add_user_to_group/use_case.rs
pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), anyhow::Error> {
    // ❌ Error genérico - consumidor no sabe qué puede fallar
}
```

**Impacto:**
- Oculta posibles fallos de la operación
- Consumidor no puede manejar errores programáticamente
- Obliga a tratar errores como strings (frágil)
- Viola el principio de "hacer explícito lo implícito"

**Solución:**
```rust
// crates/hodei-iam/src/features/add_user_to_group/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddUserToGroupError {
    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Group not found: {0}")]
    GroupNotFound(String),

    #[error("Invalid user HRN: {0}")]
    InvalidUserHrn(String),

    #[error("Invalid group HRN: {0}")]
    InvalidGroupHrn(String),

    #[error("User already in group")]
    UserAlreadyInGroup,

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),
}

// crates/hodei-iam/src/features/add_user_to_group/use_case.rs
pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), AddUserToGroupError> {
    let user_hrn = Hrn::from_string(&cmd.user_hrn)
        .ok_or_else(|| AddUserToGroupError::InvalidUserHrn(cmd.user_hrn.clone()))?;
    
    let user = self.user_finder
        .find_by_hrn(&user_hrn)
        .await
        .map_err(|e| AddUserToGroupError::RepositoryError(e.to_string()))?
        .ok_or_else(|| AddUserToGroupError::UserNotFound(cmd.user_hrn.clone()))?;
    
    // ... resto de la lógica
}
```

---

## 📅 Plan de Implementación (Priorizado)

### Fase 1: Preparación y Fundamentos (2-3 días)

#### Tarea 1.1: Consolidar Kernel Compartido
- [ ] Mover tipos compartidos de `hodei-iam/src/shared` → `kernel/`
- [ ] Mover tipos compartidos de `hodei-organizations/src/shared` → `kernel/`
- [ ] Definir traits transversales en `kernel/application/ports/`
- [ ] Actualizar dependencias en `Cargo.toml`

**Resultado esperado:**
```
crates/kernel/
├── domain/
│   ├── hrn.rs                  ✅ Tipo canónico compartido
│   ├── aggregate.rs            ✅ Trait base
│   ├── id.rs                   ✅ Value objects compartidos
│   └── events.rs               ✅ Eventos de dominio compartidos
└── application/
    └── ports/
        ├── auth.rs             ✅ AuthContextProvider
        └── effective_policies.rs ✅ EffectivePoliciesQueryPort
```

#### Tarea 1.2: Refactorizar `hodei-iam` - Encapsulamiento
- [ ] Renombrar `src/shared/` → `src/internal/`
- [ ] Hacer `internal` privado en `lib.rs`
- [ ] Remover exportaciones públicas de `infrastructure` y `ports`
- [ ] Actualizar `lib.rs` para exportar solo features

**Resultado esperado:**
```rust
// crates/hodei-iam/src/lib.rs
mod internal;  // ✅ Privado

pub mod features;

// Re-exportar solo casos de uso
pub use features::create_user::{CreateUserUseCase, CreateUserCommand, CreateUserError};
pub use features::add_user_to_group::{AddUserToGroupUseCase, AddUserToGroupCommand, AddUserToGroupError};
// ...
```

#### Tarea 1.3: Refactorizar `hodei-organizations` - Encapsulamiento
- [ ] Renombrar `src/shared/` → `src/internal/`
- [ ] Hacer `internal` privado en `lib.rs`
- [ ] Exportar solo features públicas
- [ ] Actualizar dependencias externas

---

### Fase 2: Segregación de Features (3-4 días)

#### Tarea 2.1: Dividir `create_policy` en Features Independientes

**Features a crear:**
1. `create_policy/`
2. `delete_policy/`
3. `update_policy/`
4. `get_policy/`
5. `list_policies/`

**Para cada feature:**
- [ ] Crear estructura VSA completa
- [ ] Definir puerto segregado específico
- [ ] Implementar caso de uso
- [ ] Crear DTOs específicos
- [ ] Crear error específico con `thiserror`
- [ ] Implementar adaptador
- [ ] Escribir tests unitarios con mocks
- [ ] Configurar DI

**Estructura objetivo por feature:**
```
features/create_policy/
├── mod.rs
├── use_case.rs              ✅ CreatePolicyUseCase
├── ports.rs                 ✅ CreatePolicyPort (solo create)
├── dto.rs                   ✅ CreatePolicyCommand
├── error.rs                 ✅ CreatePolicyError
├── adapter.rs               ✅ SurrealCreatePolicyAdapter
├── use_case_test.rs         ✅ Tests unitarios
├── mocks.rs                 ✅ MockCreatePolicyPort
└── di.rs                    ✅ DI config
```

#### Tarea 2.2: Aplicar ISP a Puertos de Repositorio

**Antes (monolítico):**
```rust
pub trait PolicyRepository {
    fn create(&self, policy: Policy) -> Result<()>;
    fn delete(&self, id: &str) -> Result<()>;
    fn update(&self, policy: Policy) -> Result<()>;
    fn get(&self, id: &str) -> Result<Option<Policy>>;
    fn list(&self) -> Result<Vec<Policy>>;
}
```

**Después (segregado):**
```rust
// features/create_policy/ports.rs
pub trait CreatePolicyPort {
    fn create(&self, policy: Policy) -> Result<()>;
}

// features/delete_policy/ports.rs
pub trait DeletePolicyPort {
    fn delete(&self, id: &str) -> Result<()>;
}

// features/get_policy/ports.rs
pub trait GetPolicyPort {
    fn get(&self, id: &str) -> Result<Option<Policy>>;
}
```

---

### Fase 3: Errores Específicos (1-2 días)

#### Tarea 3.1: Reemplazar `anyhow::Error` en `hodei-iam`

**Features a actualizar:**
- [ ] `add_user_to_group`
- [ ] `create_group`
- [ ] `create_user`

**Para cada feature:**
- [ ] Crear `error.rs` con enum específico
- [ ] Actualizar firma de `execute()` en `use_case.rs`
- [ ] Mapear errores internos al tipo específico
- [ ] Actualizar tests para verificar errores específicos

**Template de error:**
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum [Feature]Error {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Invalid HRN: {0}")]
    InvalidHrn(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),
}
```

---

### Fase 4: Desacoplamiento Infraestructura/Aplicación (2 días)

#### Tarea 4.1: Refactorizar `SurrealOrganizationBoundaryProvider`

**Problema actual:**
```rust
// ❌ Infraestructura llama a caso de uso
let use_case = get_effective_scps_use_case(...);
use_case.execute(...).await
```

**Solución:**
- [ ] Inyectar repositorios en constructor del adaptador
- [ ] Implementar lógica de negocio directamente en el adaptador
- [ ] Eliminar dependencia del caso de uso

**Nuevas dependencias del adaptador:**
```rust
pub struct SurrealOrganizationBoundaryProvider {
    account_repo: Arc<dyn AccountRepository>,
    ou_repo: Arc<dyn OuRepository>,
    scp_repo: Arc<dyn ScpRepository>,
}
```

---

### Fase 5: Tests de Integración (3-4 días)

#### Tarea 5.1: Tests de Integración por Bounded Context

**Para cada crate (`hodei-iam`, `hodei-organizations`, `hodei-authorizer`):**

**Estructura de tests:**
```
crates/hodei-iam/tests/
├── integration/
│   ├── create_user_integration_test.rs
│   ├── add_user_to_group_integration_test.rs
│   ├── create_policy_integration_test.rs
│   └── ...
├── compose/
│   └── docker-compose.yml        ✅ SurrealDB para tests
└── common/
    └── mod.rs                     ✅ Test helpers
```

**Casos de test por feature:**
- [ ] Happy path (caso exitoso)
- [ ] Validación de inputs
- [ ] Manejo de errores específicos
- [ ] Persistencia en SurrealDB real
- [ ] Emisión de eventos de dominio

**Ejemplo de test:**
```rust
#[tokio::test]
async fn test_create_user_integration() {
    // Arrange
    let container = setup_surrealdb_container().await;
    let db = connect_to_test_db(&container).await;
    let use_case = make_create_user_use_case(db.clone());

    let cmd = CreateUserCommand {
        user_hrn: "hrn:hodei:iam:user:alice".to_string(),
        name: "Alice".to_string(),
    };

    // Act
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    
    // Verify persistence
    let user = db.query("SELECT * FROM user WHERE hrn = $hrn")
        .bind(("hrn", "hrn:hodei:iam:user:alice"))
        .await
        .unwrap();
    
    assert!(user.is_some());
}
```

#### Tarea 5.2: Tests End-to-End con Testcontainers

- [ ] Setup de SurrealDB con testcontainers
- [ ] Configuración de event bus (in-memory para tests)
- [ ] Tests de flujos completos cross-feature
- [ ] Verificación de eventos emitidos

**Ejemplo:**
```rust
#[tokio::test]
async fn test_e2e_user_group_policy_flow() {
    // 1. Create user
    // 2. Create group
    // 3. Add user to group
    // 4. Create policy for group
    // 5. Verify effective policies for user
}
```

---

### Fase 6: API Pública y Documentación (2 días)

#### Tarea 6.1: Definir API Pública de Cada Crate

**Para cada crate:**
- [ ] Documentar casos de uso públicos
- [ ] Documentar DTOs de entrada/salida
- [ ] Documentar errores posibles
- [ ] Ejemplos de uso

**Template de documentación:**
```rust
//! # hodei-iam
//!
//! IAM Bounded Context para el sistema Hodei Artifacts.
//!
//! ## API Pública
//!
//! Este crate expone únicamente casos de uso (features) a través de su API pública.
//! NO expone entidades de dominio, repositorios ni implementaciones de infraestructura.
//!
//! ### Features Disponibles
//!
//! - `CreateUserUseCase`: Crear un nuevo usuario
//! - `AddUserToGroupUseCase`: Añadir usuario a grupo
//! - `GetEffectivePoliciesUseCase`: Obtener políticas efectivas
//!
//! ### Ejemplo de Uso
//!
//! ```rust
//! use hodei_iam::{CreateUserUseCase, CreateUserCommand};
//!
//! let use_case = CreateUserUseCase::new(/* dependencies */);
//! let cmd = CreateUserCommand {
//!     user_hrn: "hrn:hodei:iam:user:alice".to_string(),
//!     name: "Alice".to_string(),
//! };
//!
//! let result = use_case.execute(cmd).await?;
//! ```
```

#### Tarea 6.2: Actualizar Documentación del Proyecto

- [ ] Actualizar `README.md` con nueva arquitectura
- [ ] Crear diagrama de bounded contexts
- [ ] Documentar flujo de DI
- [ ] Guía de contribución actualizada

---

## ✅ Checklist de Verificación por Fase

### Verificación Fase 1 (Encapsulamiento)
- [ ] `hodei-iam/src/internal/` es privado
- [ ] `hodei-organizations/src/internal/` es privado
- [ ] `kernel/` contiene solo tipos compartidos
- [ ] No hay exportaciones públicas de `infrastructure`
- [ ] No hay exportaciones públicas de `ports` genéricos
- [ ] Código compila sin errores
- [ ] Tests unitarios pasan

### Verificación Fase 2 (Features Segregadas)
- [ ] Cada feature CRUD es un módulo independiente
- [ ] Cada feature tiene sus propios ports (ISP)
- [ ] No hay puertos monolíticos
- [ ] Cada feature tiene error específico
- [ ] Tests unitarios por feature
- [ ] Código compila sin errores
- [ ] `cargo clippy` sin warnings

### Verificación Fase 3 (Errores Específicos)
- [ ] Ningún `anyhow::Error` en firmas públicas
- [ ] Cada feature tiene enum de error con `thiserror`
- [ ] Tests verifican errores específicos
- [ ] Documentación de errores posibles

### Verificación Fase 4 (Desacoplamiento)
- [ ] Infraestructura NO llama a casos de uso
- [ ] Adaptadores inyectan repositorios
- [ ] Flujo de dependencias correcto (App → Infra)
- [ ] Tests de adaptadores

### Verificación Fase 5 (Tests Integración)
- [ ] Tests de integración por feature
- [ ] Tests con SurrealDB real (testcontainers)
- [ ] Cobertura > 80% en casos de uso
- [ ] Tests E2E para flujos críticos
- [ ] `cargo nextest run` ejecuta todo < 2s

### Verificación Fase 6 (API y Docs)
- [ ] API pública documentada
- [ ] Ejemplos de uso por feature
- [ ] Diagramas de arquitectura
- [ ] README actualizado

---

## 🔍 Métricas de Éxito

### Métricas de Calidad
- ✅ 0 exportaciones públicas de módulos internos
- ✅ 0 warnings de `cargo clippy`
- ✅ 0 usos de `anyhow::Error` en API pública
- ✅ Cobertura de tests > 80% en casos de uso
- ✅ Todos los tests pasan en < 2 segundos

### Métricas de Arquitectura
- ✅ Todas las features siguen VSA estricta
- ✅ Todos los puertos cumplen ISP
- ✅ Kernel contiene solo tipos compartidos
- ✅ 0 acoplamientos entre bounded contexts
- ✅ 0 ciclos de dependencias

### Métricas de Testing
- ✅ Cada feature tiene tests unitarios
- ✅ Cada crate tiene tests de integración
- ✅ Tests E2E para flujos críticos
- ✅ Tests con testcontainers funcionando

---

## 📚 Referencias

### Principios Arquitectónicos
- Clean Architecture (Robert C. Martin)
- Vertical Slice Architecture (Jimmy Bogard)
- Domain-Driven Design (Eric Evans)
- SOLID Principles

### Documentos del Proyecto
- `CLAUDE.md`: Reglas arquitectónicas del proyecto
- `docs/historias-usuario.md`: Análisis de violaciones
- `TEST_COVERAGE_EXPANSION_SUMMARY.md`: Estado actual de tests

---

## 🚀 Próximos Pasos

1. **Comenzar con Fase 1** (Fundamentos y Encapsulamiento)
2. **Validar cada fase** con checklist antes de continuar
3. **Mantener tests pasando** en todo momento
4. **Documentar cambios** conforme se implementan
5. **Revisar cobertura** después de cada fase

---

**Última actualización:** 2024-01-XX  
**Responsable:** Equipo de Arquitectura Hodei Artifacts  
**Estado:** 🟡 Planificación Completa - Pendiente de Implementación