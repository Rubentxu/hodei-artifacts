# Antes/Después: Uso de Ports vs Implementaciones Directas

## 🔴 ANTES (INCORRECTO) - Estado Actual

### hodei-iam usa directamente implementaciones de hodei-policies

**`crates/hodei-iam/src/features/evaluate_iam_policies/use_case.rs`**

```rust
// ❌ INCORRECTO: Importar implementación directa
use hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase;

pub struct EvaluateIamPoliciesUseCase<PF, PR, RR>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
{
    policy_finder: Arc<PF>,
    principal_resolver: Arc<PR>,
    resource_resolver: Arc<RR>,
    
    // ❌ ACOPLAMIENTO DIRECTO: Tipo concreto en lugar de trait
    policies_evaluator: EvaluatePoliciesUseCase,
}

impl<PF, PR, RR> EvaluateIamPoliciesUseCase<PF, PR, RR>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
{
    pub fn new(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
    ) -> Self {
        Self {
            policy_finder,
            principal_resolver,
            resource_resolver,
            
            // ❌ INSTANCIACIÓN DIRECTA: hodei-iam conoce la implementación
            policies_evaluator: EvaluatePoliciesUseCase::new(),
        }
    }
}
```

### Problemas con este enfoque:

1. ❌ **Acoplamiento directo** entre bounded contexts
2. ❌ **Violación de Dependency Inversion Principle**
3. ❌ **Imposible mockear** en tests unitarios
4. ❌ **No respeta bounded context boundaries**
5. ❌ **Dificulta cambiar implementaciones**

---

## ✅ DESPUÉS (CORRECTO) - Cómo Debe Ser

### hodei-iam usa PORTS (traits) de hodei-policies

#### Paso 1: hodei-policies expone un PORT

**`crates/hodei-policies/src/features/evaluate_policies/ports.rs`** (NUEVO)

```rust
use async_trait::async_trait;
use super::dto::{EvaluatePoliciesCommand, EvaluationDecision};
use super::error::EvaluatePoliciesError;

/// Port for evaluating authorization policies
///
/// This port abstracts the policy evaluation logic, allowing bounded contexts
/// to evaluate Cedar policies without depending on concrete implementations.
#[async_trait]
pub trait EvaluatePoliciesPort: Send + Sync {
    /// Evaluate an authorization request against policies
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError>;
}
```

**`crates/hodei-policies/src/features/evaluate_policies/use_case.rs`** (MODIFICAR)

```rust
// Implementar el port en el use case
#[async_trait]
impl EvaluatePoliciesPort for EvaluatePoliciesUseCase {
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        // Delegar al método execute existente
        self.execute(command).await
    }
}
```

**`crates/hodei-policies/src/features/evaluate_policies/mod.rs`** (MODIFICAR)

```rust
pub mod ports;  // ← Exponer ports públicamente

// Re-exports
pub use ports::EvaluatePoliciesPort;
pub use use_case::EvaluatePoliciesUseCase;
```

**`crates/hodei-policies/src/api.rs`** (verificar)

```rust
// Exporta el módulo completo
pub use crate::features::evaluate_policies;
```

---

#### Paso 2: hodei-iam usa el PORT (no la implementación)

**`crates/hodei-iam/src/features/evaluate_iam_policies/ports.rs`** (MODIFICAR)

```rust
use async_trait::async_trait;
use kernel::domain::HodeiPolicySet;
use kernel::{HodeiEntity, Hrn};

// ... ports existentes (PolicyFinderPort, PrincipalResolverPort, ResourceResolverPort)

// ✅ CORRECTO: Re-exportar port de hodei-policies
pub use hodei_policies::features::evaluate_policies::EvaluatePoliciesPort;

// Re-exportar tipos necesarios para el comando
pub use hodei_policies::features::evaluate_policies::dto::{
    AuthorizationRequest,
    Decision,
    EvaluatePoliciesCommand,
    EvaluationDecision,
};
pub use hodei_policies::features::evaluate_policies::error::EvaluatePoliciesError;
```

**`crates/hodei-iam/src/features/evaluate_iam_policies/use_case.rs`** (MODIFICAR)

```rust
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

use kernel::application::ports::authorization::{
    AuthorizationError,
    EvaluationDecision as KernelEvaluationDecision,
    EvaluationRequest as KernelEvaluationRequest,
    IamPolicyEvaluator,
};

use super::ports::{
    EntityResolverError,
    EvaluatePoliciesPort,  // ✅ Usar el port
    PolicyFinderError,
    PolicyFinderPort,
    PrincipalResolverPort,
    ResourceResolverPort,
    // DTOs re-exportados
    AuthorizationRequest,
    EvaluatePoliciesCommand,
};

/// Use case for evaluating IAM policies
///
/// ✅ Ahora depende de un PORT, no de una implementación concreta
pub struct EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
    EP: EvaluatePoliciesPort,  // ✅ TRAIT, no tipo concreto
{
    policy_finder: Arc<PF>,
    principal_resolver: Arc<PR>,
    resource_resolver: Arc<RR>,
    
    // ✅ CORRECTO: Dependencia en trait, no en implementación
    policies_evaluator: Arc<EP>,
}

impl<PF, PR, RR, EP> EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
where
    PF: PolicyFinderPort,
    PR: PrincipalResolverPort,
    RR: ResourceResolverPort,
    EP: EvaluatePoliciesPort,
{
    /// ✅ Constructor ahora recibe el port inyectado
    pub fn new(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
        policies_evaluator: Arc<EP>,  // ✅ INYECTADO por DI
    ) -> Self {
        Self {
            policy_finder,
            principal_resolver,
            resource_resolver,
            policies_evaluator,
        }
    }
}

#[async_trait]
impl<PF, PR, RR, EP> IamPolicyEvaluator for EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
where
    PF: PolicyFinderPort + Send + Sync,
    PR: PrincipalResolverPort + Send + Sync,
    RR: ResourceResolverPort + Send + Sync,
    EP: EvaluatePoliciesPort + Send + Sync,
{
    #[instrument(/* ... */)]
    async fn evaluate_iam_policies(
        &self,
        request: KernelEvaluationRequest,
    ) -> Result<KernelEvaluationDecision, AuthorizationError> {
        // ... lógica de obtener políticas y entidades ...

        let evaluate_command = EvaluatePoliciesCommand {
            request: auth_request,
            policies: &policy_set,
            entities: &entities,
        };

        // ✅ CORRECTO: Usar el port (método del trait)
        let evaluation_result = self
            .policies_evaluator
            .evaluate(evaluate_command)  // ← método del trait
            .await
            .map_err(|e| {
                warn!(error = %e, "Policy evaluation failed");
                AuthorizationError::EvaluationFailed(format!("Cedar evaluation failed: {}", e))
            })?;

        // ... mapear resultado ...
    }
}
```

**`crates/hodei-iam/src/features/evaluate_iam_policies/di.rs`** (MODIFICAR)

```rust
use std::sync::Arc;
use super::ports::{PolicyFinderPort, PrincipalResolverPort, ResourceResolverPort, EvaluatePoliciesPort};
use super::use_case::EvaluateIamPoliciesUseCase;

pub struct EvaluateIamPoliciesUseCaseFactory;

impl EvaluateIamPoliciesUseCaseFactory {
    /// ✅ Factory ahora acepta el port del evaluator
    pub fn build<PF, PR, RR, EP>(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
        policies_evaluator: Arc<EP>,  // ✅ INYECTADO
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

#### Paso 3: Composition Root (main.rs) hace la DI

**`src/main.rs`** o **`src/app_state.rs`**

```rust
use std::sync::Arc;
use hodei_iam::features::evaluate_iam_policies;
use hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase;

async fn initialize_app_state(db: Arc<SurrealDB>) -> AppState {
    // 1. Instanciar adaptadores de infraestructura
    let policy_finder = Arc::new(SurrealPolicyFinderAdapter::new(db.clone()));
    let principal_resolver = Arc::new(SurrealPrincipalResolverAdapter::new(db.clone()));
    let resource_resolver = Arc::new(SurrealResourceResolverAdapter::new(db.clone()));
    
    // 2. ✅ Instanciar la implementación concreta del evaluator
    let policies_evaluator = Arc::new(EvaluatePoliciesUseCase::new());
    
    // 3. ✅ Inyectar la implementación via factory
    //    El compilador verifica que EvaluatePoliciesUseCase implementa EvaluatePoliciesPort
    let evaluate_iam_use_case = evaluate_iam_policies::di::EvaluateIamPoliciesUseCaseFactory::build(
        policy_finder,
        principal_resolver,
        resource_resolver,
        policies_evaluator,  // ✅ La implementación se pasa como Arc<dyn Trait>
    );
    
    AppState {
        evaluate_iam: Arc::new(evaluate_iam_use_case),
    }
}
```

---

## 📊 Comparación Visual

### ANTES (❌ Acoplamiento Directo)

```
┌─────────────────────┐
│   hodei-iam         │
│                     │
│  use_case.rs        │
│  ┌───────────────┐  │
│  │ EvaluateIam   │  │
│  │ UseCase       │  │
│  │               │  │
│  │ evaluator:    │  │──────┐
│  │  Evaluate     │  │      │ Importación directa
│  │  PoliciesUC   │  │      │ (acoplamiento fuerte)
│  └───────────────┘  │      │
└─────────────────────┘      │
                             │
                             ▼
              ┌──────────────────────────┐
              │   hodei-policies         │
              │                          │
              │  EvaluatePoliciesUseCase │
              │  (implementación)        │
              └──────────────────────────┘
```

### DESPUÉS (✅ Dependency Inversion)

```
┌─────────────────────┐        ┌──────────────────────────┐
│   hodei-iam         │        │   hodei-policies         │
│                     │        │                          │
│  use_case.rs        │        │  ports.rs                │
│  ┌───────────────┐  │        │  ┌────────────────────┐  │
│  │ EvaluateIam   │  │        │  │ Evaluate           │  │
│  │ UseCase<EP>   │  │◄───────┼──│ PoliciesPort       │  │
│  │               │  │ Depende│  │ (trait)            │  │
│  │ evaluator:    │  │   de   │  └────────────────────┘  │
│  │  Arc<EP>      │  │  Trait │           ▲              │
│  └───────────────┘  │        │           │              │
└─────────────────────┘        │           │ Implementa   │
         ▲                     │           │              │
         │                     │  ┌────────────────────┐  │
         │ Inyectado por DI    │  │ Evaluate           │  │
         │                     │  │ PoliciesUseCase    │  │
         │                     │  │ (implementación)   │  │
         │                     │  └────────────────────┘  │
         │                     └──────────────────────────┘
         │
         │
┌────────┴─────────────┐
│   main.rs            │
│  (composition root)  │
│                      │
│  let evaluator =     │
│    Arc::new(         │
│      EvaluateUC::new()│
│    );                │
│                      │
│  Factory::build(     │
│    ...,              │
│    evaluator         │
│  );                  │
└──────────────────────┘
```

---

## ✅ Beneficios del Enfoque con Ports

### 1. Cero Acoplamiento entre Bounded Contexts

```rust
// hodei-iam NO conoce la implementación de hodei-policies
// Solo conoce el contrato (trait)
```

### 2. Testabilidad Total

**Test con Mock:**

```rust
// En use_case_test.rs
struct MockPoliciesEvaluator {
    should_allow: bool,
}

#[async_trait]
impl EvaluatePoliciesPort for MockPoliciesEvaluator {
    async fn evaluate(&self, _: EvaluatePoliciesCommand<'_>) 
        -> Result<EvaluationDecision, EvaluatePoliciesError> 
    {
        Ok(EvaluationDecision {
            decision: if self.should_allow { Decision::Allow } else { Decision::Deny },
            determining_policies: vec![],
            reasons: vec!["Mock decision".to_string()],
        })
    }
}

#[tokio::test]
async fn test_evaluate_with_mock() {
    let mock_evaluator = Arc::new(MockPoliciesEvaluator { should_allow: true });
    
    let use_case = EvaluateIamPoliciesUseCaseFactory::build(
        // ... otros mocks ...
        mock_evaluator,  // ✅ Inyectar mock
    );
    
    // Test aislado, sin dependencias de hodei-policies
}
```

### 3. Flexibilidad

```rust
// Fácil cambiar implementación sin tocar hodei-iam
// Por ejemplo: usar un evaluator en memoria para dev
let dev_evaluator = Arc::new(InMemoryPoliciesEvaluator::new());

// O usar un evaluator con cache
let cached_evaluator = Arc::new(CachedPoliciesEvaluator::new(real_evaluator));

// hodei-iam no cambia, solo cambia la inyección en main.rs
```

### 4. Respeta Bounded Context Boundaries

```rust
// hodei-iam (IAM context)
//   ↓ depende de
// Port (contrato en hodei-policies)
//   ↑ implementado por
// hodei-policies (Policies context)

// Cada contexto mantiene su autonomía
```

### 5. Dependency Inversion Principle (SOLID)

```rust
// Módulos de alto nivel (hodei-iam) NO dependen de módulos de bajo nivel (implementaciones)
// Ambos dependen de abstracciones (traits/ports)
```

---

## 🔄 Patrón General para Todos los Bounded Contexts

### Regla de Oro

> **Si un bounded context A necesita funcionalidad de un bounded context B:**
> 
> 1. ✅ B expone un **PORT** (trait)
> 2. ✅ B implementa el port en su **USE CASE**
> 3. ✅ A depende del **PORT**, nunca de la implementación
> 4. ✅ La **composition root** (main.rs) inyecta la implementación

### Ejemplo: hodei-iam necesita validar políticas

```rust
// hodei-policies expone:
pub trait ValidatePolicyPort: Send + Sync {
    async fn validate(&self, command: ValidatePolicyCommand) 
        -> Result<ValidationResult, ValidationError>;
}

// hodei-iam usa:
use hodei_policies::features::validate_policy::ValidatePolicyPort;

pub struct CreatePolicyUseCase<V>
where
    V: ValidatePolicyPort,
{
    validator: Arc<V>,  // ← Dependencia en trait
}
```

---

## 📋 Checklist de Verificación

Para verificar que un bounded context está correctamente desacoplado:

- [ ] ✅ No hay imports directos de `UseCase` de otros BCs
- [ ] ✅ Solo se importan `Port` traits
- [ ] ✅ Los use cases dependen de `Arc<dyn Port>`
- [ ] ✅ La DI se hace en `main.rs` (composition root)
- [ ] ✅ Los tests usan mocks que implementan los ports
- [ ] ✅ Compilador verifica que implementaciones cumplen los ports

---

## 🎯 Conclusión

**ANTES**: Acoplamiento directo, difícil de testear, violación de SOLID

**DESPUÉS**: Desacoplamiento total via ports, testeable 100%, respeta DDD y bounded contexts

El patrón de **Ports & Adapters** (Hexagonal Architecture) es fundamental para mantener la separación entre bounded contexts mientras se permite la colaboración necesaria para la funcionalidad del sistema.