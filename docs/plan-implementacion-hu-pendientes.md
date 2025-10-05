# Plan de Implementación - Historias de Usuario Pendientes

## 📊 Estado Actual del Proyecto

### ✅ COMPLETADAS

#### **Épica 1: Contratos y Límites Arquitectónicos**
- ✅ **HU-1.1**: Abstracciones centralizadas en `kernel` (antes `shared`)
  - `Hrn`, `HodeiEntity`, `Principal`, `Resource`, `ActionTrait` están en `kernel/src/domain`
- ✅ **HU-1.2**: Módulos `shared` privados en bounded contexts
  - `hodei-iam/src/lib.rs`: `mod shared;` (privado)
  - `hodei-organizations/src/lib.rs`: `mod shared;` (privado)
  - Solo se exportan eventos, puertos e infraestructura específica
- ✅ **HU-1.3**: Traits de evaluación definidos en `kernel`
  - `ScpEvaluator` y `IamPolicyEvaluator` en `kernel/src/application/ports/authorization.rs`

#### **Épica 2: Simplificar `policies` a Biblioteca Pura**
- ✅ **HU-2.1**: Features CRUD eliminadas
  - Solo quedan: `validate_policy`, `policy_playground`, `policy_analysis`, `batch_eval`
- ✅ **HU-2.2**: Capa de persistencia eliminada
  - No existe `PolicyStore` ni `PolicyStorage`
  - `policies` solo tiene `EngineBuilder` para construir esquemas Cedar

#### **Épica 4: Simplificar `hodei-authorizer` (Parcial)**
- ✅ **HU-4.1**: `EvaluatePermissionsUseCase` es orquestador puro
  - Depende de `Arc<dyn IamPolicyEvaluator>` y `Arc<dyn ScpEvaluator>`
  - No tiene lógica de búsqueda de políticas

---

### ❌ PENDIENTES (CRÍTICAS)

## 🎯 FASE 1: Épica 3 - Implementar Evaluadores en Dominios Autónomos

### **HU-3.2: hodei-iam gestiona y evalúa sus propias políticas** 🔴 ALTA PRIORIDAD

**Objetivo:** Crear la feature `evaluate_iam_policies` que implementa `IamPolicyEvaluator`

#### **Estructura de Directorios**
```
crates/hodei-iam/src/features/evaluate_iam_policies/
├── mod.rs              # Exporta la feature
├── use_case.rs         # Lógica principal - implementa IamPolicyEvaluator
├── ports.rs            # Traits: PolicyFinderPort, CedarEnginePort
├── adapter.rs          # Implementaciones concretas
├── error.rs            # EvaluateIamPoliciesError
├── di.rs               # Configuración DI
├── use_case_test.rs    # Tests unitarios con mocks
└── mocks.rs            # Mocks para tests
```

#### **Tareas Detalladas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 3.2.1 | Crear estructura de directorios | `features/evaluate_iam_policies/` | Crear carpeta y archivos base |
| 3.2.2 | Definir puertos | `ports.rs` | `PolicyFinderPort`, `CedarEnginePort` |
| 3.2.3 | Definir errores | `error.rs` | `EvaluateIamPoliciesError` con variantes |
| 3.2.4 | Implementar caso de uso | `use_case.rs` | Implementar trait `IamPolicyEvaluator` |
| 3.2.5 | Implementar adaptadores | `adapter.rs` | `PolicyRepositoryAdapter`, `CedarEvaluatorAdapter` |
| 3.2.6 | Crear mocks | `mocks.rs` | `MockPolicyFinder`, `MockCedarEngine` |
| 3.2.7 | Escribir tests unitarios | `use_case_test.rs` | Tests con coverage >80% |
| 3.2.8 | Configurar DI | `di.rs` | Función `make_iam_policy_evaluator()` |
| 3.2.9 | Exportar desde mod.rs | `features/evaluate_iam_policies/mod.rs` | Exportar públicamente |
| 3.2.10 | Actualizar lib.rs | `hodei-iam/src/lib.rs` | Re-exportar feature |

#### **Código de Ejemplo**

**ports.rs:**
```rust
use kernel::domain::Hrn;
use kernel::application::ports::authorization::{EvaluationRequest, EvaluationDecision, AuthorizationError};

/// Puerto para obtener políticas IAM de un principal
pub trait PolicyFinderPort: Send + Sync {
    async fn get_policies_for_principal(
        &self, 
        principal_hrn: &Hrn
    ) -> Result<Vec<String>, AuthorizationError>;
}

/// Puerto para evaluar con Cedar
pub trait CedarEnginePort: Send + Sync {
    fn evaluate(
        &self,
        policy_set: &str,
        request: &EvaluationRequest
    ) -> Result<EvaluationDecision, AuthorizationError>;
}
```

**use_case.rs:**
```rust
use std::sync::Arc;
use async_trait::async_trait;
use kernel::application::ports::authorization::{
    IamPolicyEvaluator, EvaluationRequest, EvaluationDecision, AuthorizationError
};
use super::ports::{PolicyFinderPort, CedarEnginePort};

pub struct EvaluateIamPoliciesUseCase<PF, CE> 
where
    PF: PolicyFinderPort,
    CE: CedarEnginePort,
{
    policy_finder: Arc<PF>,
    cedar_engine: Arc<CE>,
}

impl<PF, CE> EvaluateIamPoliciesUseCase<PF, CE>
where
    PF: PolicyFinderPort,
    CE: CedarEnginePort,
{
    pub fn new(policy_finder: Arc<PF>, cedar_engine: Arc<CE>) -> Self {
        Self { policy_finder, cedar_engine }
    }
}

#[async_trait]
impl<PF, CE> IamPolicyEvaluator for EvaluateIamPoliciesUseCase<PF, CE>
where
    PF: PolicyFinderPort,
    CE: CedarEnginePort,
{
    async fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest
    ) -> Result<EvaluationDecision, AuthorizationError> {
        // 1. Obtener políticas efectivas del principal
        let policies = self.policy_finder
            .get_policies_for_principal(&request.principal.hrn())
            .await?;
        
        if policies.is_empty() {
            return Ok(EvaluationDecision {
                principal_hrn: request.principal.hrn().clone(),
                action_name: request.action.name().to_string(),
                resource_hrn: request.resource.hrn().clone(),
                decision: false,
                reason: "No IAM policies found for principal (implicit deny)".to_string(),
            });
        }
        
        // 2. Combinar políticas en un PolicySet
        let policy_set = policies.join("\n");
        
        // 3. Evaluar con el motor Cedar
        self.cedar_engine.evaluate(&policy_set, &request)
    }
}
```

**adapter.rs:**
```rust
use std::sync::Arc;
use async_trait::async_trait;
use cedar_policy::{Authorizer, Context, Decision, Entities, EntityUid, PolicySet, Request, Schema};
use kernel::application::ports::authorization::{AuthorizationError, EvaluationDecision, EvaluationRequest};
use super::ports::{PolicyFinderPort, CedarEnginePort};
use crate::ports::PolicyRepository;

/// Adaptador que usa el repositorio de políticas existente
pub struct PolicyRepositoryAdapter {
    policy_repo: Arc<dyn PolicyRepository>,
}

impl PolicyRepositoryAdapter {
    pub fn new(policy_repo: Arc<dyn PolicyRepository>) -> Self {
        Self { policy_repo }
    }
}

#[async_trait]
impl PolicyFinderPort for PolicyRepositoryAdapter {
    async fn get_policies_for_principal(
        &self,
        principal_hrn: &kernel::domain::Hrn
    ) -> Result<Vec<String>, AuthorizationError> {
        // 1. Obtener políticas directas del principal
        let direct_policies = self.policy_repo
            .find_by_principal(principal_hrn)
            .await
            .map_err(|e| AuthorizationError::EvaluationFailed(format!("Policy lookup failed: {}", e)))?;
        
        // 2. Si es un usuario, obtener políticas de sus grupos
        // TODO: Implementar lógica de grupos cuando sea necesario
        
        Ok(direct_policies.into_iter().map(|p| p.document).collect())
    }
}

/// Adaptador que usa Cedar directamente
pub struct CedarEvaluatorAdapter {
    schema: Arc<Schema>,
}

impl CedarEvaluatorAdapter {
    pub fn new(schema: Arc<Schema>) -> Self {
        Self { schema }
    }
}

impl CedarEnginePort for CedarEvaluatorAdapter {
    fn evaluate(
        &self,
        policy_set_str: &str,
        request: &EvaluationRequest
    ) -> Result<EvaluationDecision, AuthorizationError> {
        // Parse policy set
        let policy_set = PolicySet::from_str(policy_set_str)
            .map_err(|e| AuthorizationError::InvalidPolicyFormat)?;
        
        // Build Cedar request
        let principal = EntityUid::from_str(&request.principal.hrn().to_string())
            .map_err(|_| AuthorizationError::EvaluationFailed("Invalid principal HRN".to_string()))?;
        
        let action = EntityUid::from_str(&format!("Action::\"{}\"", request.action.name()))
            .map_err(|_| AuthorizationError::EvaluationFailed("Invalid action".to_string()))?;
        
        let resource = EntityUid::from_str(&request.resource.hrn().to_string())
            .map_err(|_| AuthorizationError::EvaluationFailed("Invalid resource HRN".to_string()))?;
        
        let cedar_request = Request::new(
            Some(principal.clone()),
            Some(action.clone()),
            Some(resource.clone()),
            Context::empty(),
        );
        
        // Evaluate
        let authorizer = Authorizer::new();
        let entities = Entities::empty();
        let response = authorizer.is_authorized(&cedar_request, &policy_set, &entities);
        
        let (decision, reason) = match response.decision() {
            Decision::Allow => (true, "Allowed by IAM policy".to_string()),
            Decision::Deny => (false, format!("Denied by IAM policy: {:?}", response.diagnostics().errors().collect::<Vec<_>>())),
        };
        
        Ok(EvaluationDecision {
            principal_hrn: request.principal.hrn().clone(),
            action_name: request.action.name().to_string(),
            resource_hrn: request.resource.hrn().clone(),
            decision,
            reason,
        })
    }
}
```

**di.rs:**
```rust
use std::sync::Arc;
use cedar_policy::Schema;
use kernel::application::ports::authorization::IamPolicyEvaluator;
use super::{
    use_case::EvaluateIamPoliciesUseCase,
    adapter::{PolicyRepositoryAdapter, CedarEvaluatorAdapter},
};
use crate::ports::PolicyRepository;

pub fn make_iam_policy_evaluator(
    policy_repo: Arc<dyn PolicyRepository>,
    schema: Arc<Schema>
) -> Arc<dyn IamPolicyEvaluator> {
    let policy_finder = Arc::new(PolicyRepositoryAdapter::new(policy_repo));
    let cedar_engine = Arc::new(CedarEvaluatorAdapter::new(schema));
    
    Arc::new(EvaluateIamPoliciesUseCase::new(policy_finder, cedar_engine))
}
```

#### **Criterios de Aceptación**
- [ ] Estructura de directorios VSA creada
- [ ] Puertos segregados en `ports.rs`
- [ ] Caso de uso implementa `IamPolicyEvaluator`
- [ ] Adaptadores concretos implementados
- [ ] Tests unitarios con >80% coverage
- [ ] Código compila sin errores
- [ ] No hay warnings
- [ ] Tests pasan (`cargo nextest run`)

---

### **HU-3.1: hodei-organizations gestiona y evalúa sus propios SCPs** 🔴 ALTA PRIORIDAD

**Objetivo:** Crear la feature `evaluate_scps` que implementa `ScpEvaluator`

#### **Estructura de Directorios**
```
crates/hodei-organizations/src/features/evaluate_scps/
├── mod.rs              # Exporta la feature
├── use_case.rs         # Lógica principal - implementa ScpEvaluator
├── ports.rs            # Traits: ScpFinderPort, HierarchyResolverPort, CedarEnginePort
├── adapter.rs          # Implementaciones concretas
├── error.rs            # EvaluateScpsError
├── di.rs               # Configuración DI
├── use_case_test.rs    # Tests unitarios con mocks
└── mocks.rs            # Mocks para tests
```

#### **Tareas Detalladas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 3.1.1 | Crear estructura de directorios | `features/evaluate_scps/` | Crear carpeta y archivos base |
| 3.1.2 | Definir puertos | `ports.rs` | `ScpFinderPort`, `HierarchyResolverPort`, `CedarEnginePort` |
| 3.1.3 | Definir errores | `error.rs` | `EvaluateScpsError` |
| 3.1.4 | Implementar caso de uso | `use_case.rs` | Implementar trait `ScpEvaluator` |
| 3.1.5 | Implementar adaptadores | `adapter.rs` | Adapters para repositorios existentes |
| 3.1.6 | Crear mocks | `mocks.rs` | Mocks para testing |
| 3.1.7 | Escribir tests unitarios | `use_case_test.rs` | Tests con coverage >80% |
| 3.1.8 | Configurar DI | `di.rs` | Función `make_scp_evaluator()` |
| 3.1.9 | Exportar desde mod.rs | `features/evaluate_scps/mod.rs` | Exportar públicamente |
| 3.1.10 | Actualizar lib.rs | `hodei-organizations/src/lib.rs` | Re-exportar feature |

#### **Código de Ejemplo**

**ports.rs:**
```rust
use kernel::domain::Hrn;
use kernel::application::ports::authorization::{EvaluationRequest, EvaluationDecision, AuthorizationError};

/// Puerto para obtener SCPs efectivos de una cuenta/OU
pub trait ScpFinderPort: Send + Sync {
    async fn get_effective_scps_for_resource(
        &self,
        resource_hrn: &Hrn
    ) -> Result<Vec<String>, AuthorizationError>;
}

/// Puerto para resolver jerarquía organizacional
pub trait HierarchyResolverPort: Send + Sync {
    async fn get_account_path(
        &self,
        resource_hrn: &Hrn
    ) -> Result<Vec<String>, AuthorizationError>;
}

/// Puerto para evaluar con Cedar
pub trait CedarEnginePort: Send + Sync {
    fn evaluate(
        &self,
        policy_set: &str,
        request: &EvaluationRequest
    ) -> Result<EvaluationDecision, AuthorizationError>;
}
```

**use_case.rs:**
```rust
use std::sync::Arc;
use async_trait::async_trait;
use kernel::application::ports::authorization::{
    ScpEvaluator, EvaluationRequest, EvaluationDecision, AuthorizationError
};
use super::ports::{ScpFinderPort, CedarEnginePort};

pub struct EvaluateScpsUseCase<SF, CE> 
where
    SF: ScpFinderPort,
    CE: CedarEnginePort,
{
    scp_finder: Arc<SF>,
    cedar_engine: Arc<CE>,
}

impl<SF, CE> EvaluateScpsUseCase<SF, CE>
where
    SF: ScpFinderPort,
    CE: CedarEnginePort,
{
    pub fn new(scp_finder: Arc<SF>, cedar_engine: Arc<CE>) -> Self {
        Self { scp_finder, cedar_engine }
    }
}

#[async_trait]
impl<SF, CE> ScpEvaluator for EvaluateScpsUseCase<SF, CE>
where
    SF: ScpFinderPort,
    CE: CedarEnginePort,
{
    async fn evaluate_scps(
        &self,
        request: EvaluationRequest
    ) -> Result<EvaluationDecision, AuthorizationError> {
        // 1. Obtener SCPs efectivos para el recurso
        let scps = self.scp_finder
            .get_effective_scps_for_resource(&request.resource.hrn())
            .await?;
        
        if scps.is_empty() {
            // Sin SCPs = implicit allow (SCPs solo restringen, no otorgan permisos)
            return Ok(EvaluationDecision {
                principal_hrn: request.principal.hrn().clone(),
                action_name: request.action.name().to_string(),
                resource_hrn: request.resource.hrn().clone(),
                decision: true,
                reason: "No SCPs apply (implicit allow)".to_string(),
            });
        }
        
        // 2. Combinar SCPs
        let policy_set = scps.join("\n");
        
        // 3. Evaluar con Cedar
        // Nota: SCPs son restrictivos, un deny en cualquier SCP niega el acceso
        let result = self.cedar_engine.evaluate(&policy_set, &request)?;
        
        Ok(result)
    }
}
```

#### **Criterios de Aceptación**
- [ ] Estructura de directorios VSA creada
- [ ] Puertos segregados en `ports.rs`
- [ ] Caso de uso implementa `ScpEvaluator`
- [ ] Adaptadores usan `get_effective_scps` existente
- [ ] Tests unitarios con >80% coverage
- [ ] Código compila sin errores
- [ ] No hay warnings
- [ ] Tests pasan

---

## 🎯 FASE 2: Épica 4 - Limpieza de `hodei-authorizer`

### **HU-4.1 (Limpieza): Eliminar código legacy**

#### **Tareas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 4.1.1 | Eliminar authorizer.rs | `crates/hodei-authorizer/src/authorizer.rs` | Archivo legacy obsoleto |
| 4.1.2 | Eliminar contracts.rs | `crates/hodei-authorizer/src/contracts.rs` | Si es legacy y no se usa |
| 4.1.3 | Limpiar Cargo.toml | `crates/hodei-authorizer/Cargo.toml` | Eliminar deps a hodei-iam/organizations si existen |
| 4.1.4 | Actualizar lib.rs | `crates/hodei-authorizer/src/lib.rs` | Eliminar re-exports obsoletos |
| 4.1.5 | Verificar compilación | Todo el workspace | `cargo check --all` |
| 4.1.6 | Verificar tests | Todo el workspace | `cargo nextest run` |

#### **Comando de Limpieza**
```bash
# Eliminar archivos obsoletos
rm crates/hodei-authorizer/src/authorizer.rs
rm crates/hodei-authorizer/src/contracts.rs  # Si aplica

# Verificar
cargo check --all
cargo clippy --all
cargo nextest run
```

---

## 🎯 FASE 3: Épica 5 - Composición en el Binario Principal

### **HU-5.1: Simplificar AppState**

#### **Objetivo**
Refactorizar `AppState` para que solo contenga use cases de las APIs expuestas.

#### **Tareas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 5.1.1 | Eliminar use cases de policies CRUD | `src/app_state.rs` | Eliminar create_policy_uc, get_policy_uc, etc. |
| 5.1.2 | Eliminar authorization_engine | `src/app_state.rs` | Ya no es necesario en AppState |
| 5.1.3 | Eliminar repositorios directos | `src/app_state.rs` | Eliminar user_repo, group_repo |
| 5.1.4 | Añadir authorizer_uc | `src/app_state.rs` | `Arc<EvaluatePermissionsUseCase>` |
| 5.1.5 | Mantener use cases válidos | `src/app_state.rs` | Solo: validate_policy, playground, analysis, batch_eval |
| 5.1.6 | Añadir use cases de IAM | `src/app_state.rs` | create_user_uc, create_group_uc, etc. |
| 5.1.7 | Añadir use cases de Orgs | `src/app_state.rs` | create_account_uc, create_ou_uc, create_scp_uc |

#### **Código de Ejemplo**

**src/app_state.rs (refactorizado):**
```rust
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub metrics: AppMetrics,
    pub health: Arc<RwLock<HealthStatus>>,
    
    // Authorization (orquestador principal)
    pub authorizer_uc: Arc<hodei_authorizer::EvaluatePermissionsUseCase<
        NoOpCache,
        ConsoleLogger,
        PrometheusMetrics
    >>,
    
    // Policy schema tools (no CRUD)
    pub validate_policy_uc: Arc<policies::features::validate_policy::use_case::ValidatePolicyUseCase>,
    pub policy_playground_uc: Arc<policies::features::policy_playground::use_case::PolicyPlaygroundUseCase>,
    pub analyze_policies_uc: Arc<policies::features::policy_analysis::use_case::AnalyzePoliciesUseCase>,
    pub batch_eval_uc: Arc<policies::features::batch_eval::use_case::BatchEvalUseCase>,
    
    // IAM use cases
    pub create_user_uc: Arc<hodei_iam::CreateUserUseCase<...>>,
    pub create_group_uc: Arc<hodei_iam::CreateGroupUseCase<...>>,
    pub add_user_to_group_uc: Arc<hodei_iam::AddUserToGroupUseCase<...>>,
    pub create_iam_policy_uc: Arc<hodei_iam::CreatePolicyUseCase<...>>,  // NUEVO
    
    // Organizations use cases
    pub create_account_uc: Arc<hodei_organizations::CreateAccountUseCase<...>>,
    pub create_ou_uc: Arc<hodei_organizations::CreateOuUseCase<...>>,
    pub create_scp_uc: Arc<hodei_organizations::CreateScpUseCase<...>>,
    pub attach_scp_uc: Arc<hodei_organizations::AttachScpUseCase<...>>,
    
    // Event bus
    pub event_bus: Arc<InMemoryEventBus>,
    
    // Audit
    pub audit_store: Arc<AuditLogStore>,
}
```

---

### **HU-5.2: Implementar Composition Root en `build_app_state`**

#### **Objetivo**
Crear la función `build_app_state` que ensambla toda la aplicación siguiendo DI.

#### **Tareas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 5.2.1 | Crear módulo di_config.rs | `src/di_config.rs` | Configuración centralizada de DI |
| 5.2.2 | Crear función build_app_state | `src/di_config.rs` | Composition root principal |
| 5.2.3 | Instanciar repositorios | `build_app_state()` | Para IAM y Organizations |
| 5.2.4 | Construir Schema global | `build_app_state()` | Usando EngineBuilder |
| 5.2.5 | Crear evaluadores de dominio | `build_app_state()` | iam_evaluator, scp_evaluator |
| 5.2.6 | Crear authorizer_uc | `build_app_state()` | Inyectar evaluadores |
| 5.2.7 | Crear use cases de gestión | `build_app_state()` | Todos los use cases necesarios |
| 5.2.8 | Actualizar main.rs | `src/main.rs` | Usar build_app_state |

#### **Código de Ejemplo**

**src/di_config.rs:**
```rust
use std::sync::Arc;
use cedar_policy::Schema;
use crate::app_state::AppState;

pub async fn build_app_state(config: Config) -> Result<AppState, AppError> {
    // 1. Inicializar infraestructura base
    let db = initialize_database(&config).await?;
    let event_bus = Arc::new(InMemoryEventBus::new());
    let audit_store = Arc::new(AuditLogStore::new());
    
    // 2. Crear repositorios de IAM
    let user_repo = Arc::new(SurrealUserRepository::new(db.clone()));
    let group_repo = Arc::new(SurrealGroupRepository::new(db.clone()));
    let iam_policy_repo = Arc::new(SurrealPolicyRepository::new(db.clone()));
    
    // 3. Crear repositorios de Organizations
    let account_repo = Arc::new(SurrealAccountRepository::new(db.clone()));
    let ou_repo = Arc::new(SurrealOuRepository::new(db.clone()));
    let scp_repo = Arc::new(SurrealScpRepository::new(db.clone()));
    
    // 4. Construir Schema global de Cedar
    let schema = Arc::new(build_global_schema()?);
    
    // 5. Crear evaluadores de dominio
    let iam_evaluator = hodei_iam::di::make_iam_policy_evaluator(
        iam_policy_repo.clone(),
        schema.clone()
    );
    
    let scp_evaluator = hodei_organizations::di::make_scp_evaluator(
        scp_repo.clone(),
        schema.clone()
    );
    
    // 6. Crear authorizer (orquestador)
    let authorizer_uc = Arc::new(
        hodei_authorizer::EvaluatePermissionsUseCase::new(
            iam_evaluator,
            scp_evaluator,
            None,  // cache
            ConsoleLogger,
            PrometheusMetrics::new()
        )
    );
    
    // 7. Crear use cases de gestión IAM
    let create_user_uc = hodei_iam::features::create_user::di::make_use_case(
        user_repo.clone(),
        event_bus.clone()
    );
    
    let create_group_uc = hodei_iam::features::create_group::di::make_use_case(
        group_repo.clone(),
        event_bus.clone()
    );
    
    let create_iam_policy_uc = hodei_iam::features::create_policy::di::make_use_case(
        iam_policy_repo.clone(),
        event_bus.clone()
    );
    
    // 8. Crear use cases de gestión Organizations
    let create_account_uc = hodei_organizations::features::create_account::di::make_use_case(
        account_repo.clone(),
        event_bus.clone()
    );
    
    let create_scp_uc = hodei_organizations::features::create_scp::di::make_use_case(
        scp_repo.clone(),
        event_bus.clone()
    );
    
    // 9. Crear use cases de tools (policies)
    let validate_policy_uc = policies::features::validate_policy::di::make_use_case(schema.clone());
    let playground_uc = policies::features::policy_playground::di::make_use_case(schema.clone());
    
    // 10. Ensamblar AppState
    Ok(AppState {
        config,
        metrics: AppMetrics::new(),
        health: Arc::new(RwLock::new(HealthStatus::new())),
        authorizer_uc,
        validate_policy_uc,
        policy_playground_uc,
        // ... resto de use cases
        event_bus,
        audit_store,
    })
}

fn build_global_schema() -> Result<Schema, AppError> {
    let mut builder = policies::EngineBuilder::new();
    
    // Registrar entidades de IAM
    hodei_iam::configure_default_iam_entities(&mut builder)?;
    
    // Registrar entidades de Organizations
    hodei_organizations::configure_organization_entities(&mut builder)?;
    
    // Construir schema
    builder.build_schema()
        .map_err(|e| AppError::SchemaError(e.to_string()))
}
```

**src/main.rs:**
```rust
mod di_config;

#[tokio::main]
async fn main() -> Result<()> {
    // Cargar configuración
    let config = Config::from_env()?;
    
    // Construir estado de la aplicación (composition root)
    let app_state = di_config::build_app_state(config).await?;
    
    // Construir router
    let app = build_router(Arc::new(app_state));
    
    // Iniciar servidor
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

---

### **HU-5.3: Reorganizar Endpoints por Dominios**

#### **Objetivo**
Eliminar `policy_handlers.rs` y crear handlers organizados por dominio.

#### **Tareas**

| ID | Tarea | Archivo | Descripción |
|----|-------|---------|-------------|
| 5.3.1 | Eliminar policy_handlers.rs | `src/api/policy_handlers.rs` | Archivo legacy |
| 5.3.2 | Eliminar policy_handlers_test.rs | `src/api/policy_handlers_test.rs` | Tests obsoletos |
| 5.3.3 | Crear handlers IAM | `src/api/iam_handlers.rs` | POST /iam/users, /iam/groups, /iam/policies |
| 5.3.4 | Crear handlers Organizations | `src/api/organizations_handlers.rs` | POST /organizations/accounts, /scps |
| 5.3.5 | Crear handlers Authorization | `src/api/authorization_handlers.rs` | POST /authorize |
| 5.3.6 | Mantener handlers de tools | `src/api/policy_tools_handlers.rs` | Validate, playground, analysis |
| 5.3.7 | Actualizar router | `src/lib.rs` o `src/router.rs` | Rutas organizadas por dominio |
| 5.3.8 | Crear tests de handlers | `src/api/*_test.rs` | Tests de cada handler |

#### **Estructura Propuesta**
```
src/api/
├── mod.rs                          # Re-exports
├── authorization_handlers.rs       # POST /authorize
├── iam_handlers.rs                 # POST /iam/{users,groups,policies}
├── organizations_handlers.rs       # POST /organizations/{accounts,ous,scps}
├── policy_tools_handlers.rs        # POST /tools/policies/{validate,playground}
├── health_handler.rs               # GET /health (mantener)
└── metrics_handler.rs              # GET /metrics (mantener)
```

#### **Código de Ejemplo**

**src/api/authorization_handlers.rs:**
```rust
use axum::{extract::State, Json};
use std::sync::Arc;
use crate::{app_state::AppState, error::Result};

#[derive(serde::Deserialize)]
pub struct AuthorizeRequest {
    pub principal: String,
    pub action: String,
    pub resource: String,
}

#[derive(serde::Serialize)]
pub struct AuthorizeResponse {
    pub allowed: bool,
    pub reason: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/authorize",
    request_body = AuthorizeRequest,
    responses(
        (status = 200, description = "Authorization decision", body = AuthorizeResponse)
    )
)]
pub async fn authorize(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthorizeRequest>,
) -> Result<Json<AuthorizeResponse>> {
    // Convertir request a domain types
    let principal = parse_hrn(&req.principal)?;
    let action = parse_action(&req.action)?;
    let resource = parse_hrn(&req.resource)?;
    
    let auth_request = hodei_authorizer::AuthorizationRequest::new(
        principal,
        action,
        resource
    );
    
    // Ejecutar caso de uso
    let response = state.authorizer_uc.execute(auth_request).await?;
    
    Ok(Json(AuthorizeResponse {
        allowed: response.decision == hodei_authorizer::AuthorizationDecision::Allow,
        reason: response.reason,
    }))
}
```

**src/api/iam_handlers.rs:**
```rust
use axum::{extract::State, Json};
use std::sync::Arc;
use crate::{app_state::AppState, error::Result};

#[derive(serde::Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/iam/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created")
    )
)]
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<CreateUserResponse>> {
    let command = hodei_iam::features::create_user::dto::CreateUserCommand {
        username: req.username,
        email: req.email,
    };
    
    let user = state.create_user_uc.execute(command).await?;
    
    Ok(Json(CreateUserResponse {
        id: user.id.to_string(),
        hrn: user.hrn.to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/iam/policies",
    request_body = CreateIamPolicyRequest,
    responses(
        (status = 201, description = "IAM Policy created")
    )
)]
pub async fn create_iam_policy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateIamPolicyRequest>,
) -> Result<Json<CreateIamPolicyResponse>> {
    let command = hodei_iam::features::create_policy::dto::CreatePolicyCommand {
        name: req.name,
        document: req.document,
        principal_hrn: req.principal_hrn,
    };
    
    let policy = state.create_iam_policy_uc.execute(command).await?;
    
    Ok(Json(CreateIamPolicyResponse {
        id: policy.id.to_string(),
    }))
}
```

**src/api/organizations_handlers.rs:**
```rust
use axum::{extract::State, Json};
use std::sync::Arc;
use crate::{app_state::AppState, error::Result};

#[utoipa::path(
    post,
    path = "/api/v1/organizations/scps",
    request_body = CreateScpRequest,
    responses(
        (status = 201, description = "SCP created")
    )
)]
pub async fn create_scp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateScpRequest>,
) -> Result<Json<CreateScpResponse>> {
    let command = hodei_organizations::features::create_scp::dto::CreateScpCommand {
        name: req.name,
        document: req.document,
    };
    
    let scp = state.create_scp_uc.execute(command).await?;
    
    Ok(Json(CreateScpResponse {
        id: scp.id.to_string(),
        hrn: scp.hrn.to_string(),
    }))
}
```

**src/router.rs (nuevo):**
```rust
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use crate::app_state::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Authorization
        .route("/api/v1/authorize", post(crate::api::authorization_handlers::authorize))
        
        // IAM
        .route("/api/v1/iam/users", post(crate::api::iam_handlers::create_user))
        .route("/api/v1/iam/groups", post(crate::api::iam_handlers::create_group))
        .route("/api/v1/iam/policies", post(crate::api::iam_handlers::create_iam_policy))
        
        // Organizations
        .route("/api/v1/organizations/accounts", post(crate::api::organizations_handlers::create_account))
        .route("/api/v1/organizations/ous", post(crate::api::organizations_handlers::create_ou))
        .route("/api/v1/organizations/scps", post(crate::api::organizations_handlers::create_scp))
        
        // Policy Tools
        .route("/api/v1/tools/policies/validate", post(crate::api::policy_tools_handlers::validate_policy))
        .route("/api/v1/tools/policies/playground", post(crate::api::policy_tools_handlers::playground))
        
        // Health & Metrics
        .route("/health", get(crate::api::health_handler::health))
        .route("/metrics", get(crate::api::metrics_handler::metrics))
        
        .with_state(state)
}
```

---

## 📝 Checklist Final de Verificación

### Por Feature Implementada
- [ ] Estructura VSA completa
- [ ] Puertos segregados (SOLID ISP)
- [ ] Caso de uso con lógica de negocio
- [ ] Adaptadores concretos
- [ ] Errores específicos
- [ ] Tests unitarios (>80% coverage)
- [ ] Tests de integración
- [ ] Configuración DI
- [ ] Exportaciones públicas correctas
- [ ] Compilación sin errores
- [ ] Sin warnings
- [ ] Tests pasan

### Global del Proyecto
- [ ] Todos los bounded contexts son autónomos
- [ ] No hay acoplamiento entre contexts
- [ ] Kernel solo contiene elementos compartidos
- [ ] Policies es biblioteca pura sin estado
- [ ] Authorizer es orquestador puro
- [ ] AppState solo contiene use cases
- [ ] API organizada por dominios
- [ ] Composition root en build_app_state
- [ ] Cobertura de tests >80%
- [ ] Documentación actualizada

---

## 🚀 Orden de Implementación Recomendado

1. **Semana 1:** HU-3.2 (evaluate_iam_policies)
2. **Semana 1:** HU-3.1 (evaluate_scps)
3. **Semana 2:** HU-4.1 (limpieza authorizer)
4. **Semana 2:** HU-5.1 (refactorizar AppState)
5. **Semana 3:** HU-5.2 (build_app_state)
6. **Semana 3:** HU-5.3 (reorganizar endpoints)
7. **Semana 4:** Tests de integración end-to-end
8. **Semana 4:** Documentación y refinamiento

---

## 📚 Referencias

- **Documento Original:** `docs/historias-usuario.md`
- **Thread Anterior:** Rust kernel workspace refactor progress
- **Arquitectura:** `CLAUDE.md` - VSA + Hexagonal + DDD
- **Testing:** `cargo nextest run` para tests rápidos
- **Coverage:** `cargo llvm-cov` para análisis de cobertura

---

**Última actualización:** 2025-01-XX
**Estado:** 🔴 PENDIENTE - Listo para comenzar implementación