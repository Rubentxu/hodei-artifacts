# Plan de Integración: Sistema de Autorización Multi-Crate

**Fecha:** 2024-01-XX  
**Objetivo:** Integrar `hodei-authorizer`, `hodei-iam`, `hodei-organizations` y `policies` para implementar autorización multi-capa tipo AWS.

---

## 📊 Estado Actual de la Arquitectura

### Crates Existentes y sus Responsabilidades

#### 1. **`policies` - Motor de Evaluación Cedar**
**Responsabilidad:** Evaluación de políticas usando Cedar Policy Language.

**API Pública:**
```rust
// Motor de autorización
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    // Método para evaluación con PolicySet externo (KEY para orquestación)
    pub fn is_authorized_with_policy_set(
        &self,
        request: &AuthorizationRequest<'_>,
        policies: &PolicySet,
    ) -> Response
}
```

**Estado:** ✅ **Completamente implementado**

---

#### 2. **`hodei-iam` - Gestión de Identidades**
**Responsabilidad:** Usuarios, grupos, políticas de identidad.

**API Pública:**
```rust
// Caso de uso CLAVE para autorización
pub struct GetEffectivePoliciesForPrincipalUseCase;

impl GetEffectivePoliciesForPrincipalUseCase {
    pub async fn execute(&self, query: GetEffectivePoliciesQuery) 
        -> Result<EffectivePoliciesResponse, Error>
}

pub struct GetEffectivePoliciesQuery {
    pub principal_hrn: String, // "hrn:hodei:iam::user/john.doe"
}

pub struct EffectivePoliciesResponse {
    pub policies: PolicySet,      // ✅ PolicySet de Cedar
    pub principal_hrn: String,
    pub policy_count: usize,
}
```

**Estado:** 🟡 **Estructura implementada, lógica pendiente**
- ✅ Trait y DTOs definidos
- ⏳ Implementación real pendiente (devuelve PolicySet vacío)
- ⏳ Necesita conectar con repositorios

---

#### 3. **`hodei-organizations` - Estructura Organizacional**
**Responsabilidad:** OUs, cuentas, SCPs (Service Control Policies).

**API Pública:**
```rust
// Caso de uso CLAVE para autorización
pub struct GetEffectiveScpsUseCase<SRP, ORP>;

impl<SRP, ORP> GetEffectiveScpsUseCase<SRP, ORP> {
    pub async fn execute(&self, query: GetEffectiveScpsQuery) 
        -> Result<EffectiveScpsResponse, Error>
}

pub struct GetEffectiveScpsQuery {
    pub resource_hrn: String, // "hrn:hodei:s3::bucket/my-bucket"
}

pub struct EffectiveScpsResponse {
    // PolicySet de Cedar con SCPs efectivas
    pub policy_set: PolicySet,
}
```

**Estado:** ✅ **Completamente implementado**
- ✅ Lógica de recolección de SCPs desde jerarquía de OUs
- ✅ Conversión a PolicySet de Cedar
- ✅ Repositorios funcionales

---

#### 4. **`hodei-authorizer` - Orquestador de Autorización**
**Responsabilidad:** Coordinar evaluación multi-capa (SCPs + IAM).

**API Pública:**
```rust
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS>;

impl EvaluatePermissionsUseCase {
    pub async fn execute(&self, request: AuthorizationRequest) 
        -> Result<AuthorizationResponse, Error>
}

pub struct AuthorizationRequest {
    pub principal: String,  // HRN del usuario/servicio
    pub action: String,     // "s3:GetObject"
    pub resource: String,   // HRN del recurso
    pub context: Option<RequestContext>,
}

pub struct AuthorizationResponse {
    pub decision: AuthorizationDecision, // Allow/Deny
    pub determining_policies: Vec<String>,
    pub reason: String,
    pub explicit: bool,
}
```

**Estado:** 🟡 **Estructura implementada, integración pendiente**
- ✅ Orquestación básica implementada
- ✅ Usa `GetEffectivePoliciesForPrincipalUseCase`
- ✅ Usa `GetEffectiveScpsUseCase` (vía trait)
- ⏳ Entidades mock (necesita entidades reales)
- ⏳ Tests de integración pendientes

---

## 🎯 Flujo de Autorización Multi-Capa (Diseño AWS)

### Modelo de Decisión (AWS IAM)
```
1. SCPs (Organization)     → Deny tiene prioridad absoluta
2. IAM Policies (Identity)  → Explicit Deny > Explicit Allow
3. Default                  → Implicit Deny (Least Privilege)
```

### Flujo Implementado en `hodei-authorizer`

```
┌─────────────────────────────────────────────────────────────┐
│  1. AuthorizationRequest                                    │
│     - principal: "hrn:hodei:iam::user/alice"                │
│     - action: "s3:GetObject"                                │
│     - resource: "hrn:hodei:s3:account-123:bucket/data"      │
└─────────────────────────────────────────────────────────────┘
                             ↓
┌─────────────────────────────────────────────────────────────┐
│  2. Get IAM Policies                                        │
│     hodei_iam::GetEffectivePoliciesForPrincipalUseCase      │
│     → Returns: PolicySet (user + group policies)            │
└─────────────────────────────────────────────────────────────┘
                             ↓
┌─────────────────────────────────────────────────────────────┐
│  3. Get SCPs                                                │
│     hodei_organizations::GetEffectiveScpsUseCase            │
│     → Returns: PolicySet (SCPs from OU hierarchy)           │
└─────────────────────────────────────────────────────────────┘
                             ↓
┌─────────────────────────────────────────────────────────────┐
│  4. Combine PolicySets                                      │
│     - SCPs first (higher precedence)                        │
│     - IAM policies second                                   │
│     → Combined PolicySet                                    │
└─────────────────────────────────────────────────────────────┘
                             ↓
┌─────────────────────────────────────────────────────────────┐
│  5. Evaluate with Cedar                                     │
│     policies::AuthorizationEngine::is_authorized_with_policy_set│
│     → Cedar Response (Allow/Deny + Diagnostics)             │
└─────────────────────────────────────────────────────────────┘
                             ↓
┌─────────────────────────────────────────────────────────────┐
│  6. AuthorizationResponse                                   │
│     - decision: Allow/Deny                                  │
│     - determining_policies: [policy_ids]                    │
│     - reason: "..."                                         │
└─────────────────────────────────────────────────────────────┘
```

---

## ✅ Componentes Ya Implementados

### 1. Orquestación en `hodei-authorizer` ✅
**Archivo:** `crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs`

**Implementado:**
- ✅ Inyección de casos de uso de `hodei-iam` y `hodei-organizations`
- ✅ Recolección de políticas IAM
- ✅ Recolección de SCPs
- ✅ Combinación de PolicySets
- ✅ Delegación a `AuthorizationEngine`
- ✅ Aspectos transversales: cache, logging, metrics

**Código Clave:**
```rust
// Usa casos de uso, NO providers custom
iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,
org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
authorization_engine: Arc<AuthorizationEngine>,

// Orquestación
async fn evaluate_authorization(&self, request: &AuthorizationRequest) {
    // 1. Get IAM policies
    let iam_response = self.iam_use_case.execute(iam_query).await?;
    
    // 2. Get SCPs
    let scp_response = org_use_case.execute(scp_query).await?;
    
    // 3. Combine
    let combined_policies = combine(iam_response.policies, scp_response);
    
    // 4. Evaluate
    let decision = self.authorization_engine
        .is_authorized_with_policy_set(&auth_request, &combined_policies);
}
```

---

### 2. Casos de Uso en `hodei-iam` y `hodei-organizations` ✅
**Estado:** Estructura correcta, necesita completar implementación.

---

## ⏳ Trabajo Pendiente Crítico

### Fase 1: Completar Implementación de `GetEffectivePoliciesForPrincipalUseCase`

**Archivo:** `crates/hodei-iam/src/features/get_effective_policies_for_principal/use_case.rs`

**Tareas:**
1. ⏳ Inyectar repositorios reales (UserRepository, GroupRepository, PolicyRepository)
2. ⏳ Implementar lógica de recolección:
   ```rust
   async fn execute(&self, query: GetEffectivePoliciesQuery) -> Result<...> {
       // 1. Buscar usuario por HRN
       let user = self.user_repo.find_by_hrn(&principal_hrn).await?;
       
       // 2. Obtener grupos del usuario
       let groups = self.group_repo.find_by_user(&user.hrn).await?;
       
       // 3. Recolectar políticas directas del usuario
       let user_policies = self.policy_repo.find_by_principal(&user.hrn).await?;
       
       // 4. Recolectar políticas de todos los grupos
       let mut group_policies = Vec::new();
       for group in groups {
           let policies = self.policy_repo.find_by_principal(&group.hrn).await?;
           group_policies.extend(policies);
       }
       
       // 5. Combinar en PolicySet
       let policy_set = self.convert_to_policy_set(
           user_policies.into_iter()
               .chain(group_policies)
               .collect()
       )?;
       
       Ok(EffectivePoliciesResponse::new(policy_set, query.principal_hrn))
   }
   ```

3. ⏳ Definir ports:
   ```rust
   // crates/hodei-iam/src/features/get_effective_policies_for_principal/ports.rs
   
   pub trait UserFinderPort {
       async fn find_by_hrn(&self, hrn: &Hrn) 
           -> Result<Option<User>, Error>;
   }
   
   pub trait GroupFinderPort {
       async fn find_by_user(&self, user_hrn: &Hrn) 
           -> Result<Vec<Group>, Error>;
   }
   
   pub trait PolicyFinderPort {
       async fn find_by_principal(&self, principal_hrn: &Hrn) 
           -> Result<Vec<PolicyDocument>, Error>;
   }
   ```

4. ⏳ Crear adaptadores
5. ⏳ Actualizar DI

---

### Fase 2: Integración de Entidades Reales en `hodei-authorizer`

**Problema Actual:** Usa `MockHodeiEntity` en lugar de entidades reales.

**Solución:**
```rust
// Crear adaptadores que conviertan HRNs en HodeiEntity

pub struct PrincipalResolver {
    user_repo: Arc<dyn UserRepository>,
}

impl PrincipalResolver {
    pub async fn resolve(&self, hrn: &Hrn) -> Result<Box<dyn HodeiEntity>, Error> {
        match hrn.resource_type.as_str() {
            "user" => {
                let user = self.user_repo.find_by_hrn(hrn).await?;
                Ok(Box::new(user) as Box<dyn HodeiEntity>)
            }
            "service-account" => {
                // ...
            }
            _ => Err(Error::InvalidPrincipalType)
        }
    }
}
```

**Tareas:**
1. ⏳ Crear `PrincipalResolver` en `hodei-authorizer`
2. ⏳ Crear `ResourceResolver` en `hodei-authorizer`
3. ⏳ Inyectar resolvers en `EvaluatePermissionsUseCase`
4. ⏳ Reemplazar `MockHodeiEntity` con entidades reales

---

### Fase 3: Tests de Integración End-to-End

**Archivo:** `crates/hodei-authorizer/tests/integration_test.rs`

**Escenarios de Test:**

#### Test 1: Deny por SCP prevalece sobre Allow IAM
```rust
#[tokio::test]
async fn test_scp_deny_overrides_iam_allow() {
    // Setup
    let user = create_user("alice");
    let iam_policy = create_policy("allow s3:*");
    attach_policy_to_user(&user, &iam_policy);
    
    let scp = create_scp("deny s3:DeleteBucket");
    attach_scp_to_account(&account, &scp);
    
    // Execute
    let request = AuthorizationRequest {
        principal: user.hrn,
        action: "s3:DeleteBucket",
        resource: bucket.hrn,
        context: None,
    };
    
    let response = authorizer.execute(request).await.unwrap();
    
    // Assert
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(response.determining_policies.contains(&scp.id));
}
```

#### Test 2: Allow IAM con SCP no restrictivo
```rust
#[tokio::test]
async fn test_iam_allow_with_permissive_scp() {
    // SCP permite todo
    let scp = create_scp("allow *:*");
    
    // IAM permite solo GetObject
    let iam_policy = create_policy("allow s3:GetObject");
    
    // Request para GetObject debe ser Allow
    let request = AuthorizationRequest {
        principal: user.hrn,
        action: "s3:GetObject",
        resource: bucket.hrn,
        context: None,
    };
    
    let response = authorizer.execute(request).await.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
    
    // Request para PutObject debe ser Deny (no en IAM)
    let request = AuthorizationRequest {
        action: "s3:PutObject",
        // ...
    };
    
    let response = authorizer.execute(request).await.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Deny);
}
```

#### Test 3: Implicit Deny (sin políticas)
```rust
#[tokio::test]
async fn test_implicit_deny_no_policies() {
    let user = create_user_without_policies("bob");
    
    let request = AuthorizationRequest {
        principal: user.hrn,
        action: "s3:GetObject",
        resource: bucket.hrn,
        context: None,
    };
    
    let response = authorizer.execute(request).await.unwrap();
    
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(!response.explicit); // Implicit deny
    assert!(response.reason.contains("Principle of Least Privilege"));
}
```

---

## 🔄 Flujo de Dependencias entre Crates

```
┌─────────────────────────────────────────────────────────────┐
│                       hodei-authorizer                      │
│  - Orquesta la autorización multi-capa                      │
│  - NO gestiona políticas directamente                       │
│  - USA casos de uso de otros crates                         │
└─────────────────────────────────────────────────────────────┘
           │                           │
           │ uses                      │ uses
           ↓                           ↓
┌──────────────────────┐    ┌──────────────────────────────┐
│    hodei-iam         │    │  hodei-organizations         │
│  GetEffective        │    │  GetEffectiveScpsUseCase     │
│  PoliciesFor         │    │                              │
│  PrincipalUseCase    │    │  ✅ Implementado             │
│  ⏳ Pendiente         │    └──────────────────────────────┘
└──────────────────────┘
           │                           │
           │ uses                      │ uses
           ↓                           ↓
┌─────────────────────────────────────────────────────────────┐
│                          policies                           │
│  - AuthorizationEngine                                      │
│  - is_authorized_with_policy_set()                          │
│  - PolicySet, Schema, Cedar integration                     │
│  ✅ Completamente implementado                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 📝 Decisiones de Arquitectura

### ✅ Decisión 1: No usar Providers Custom
**Rechazado:** `IamPolicyProvider`, `OrganizationBoundaryProvider` traits custom.

**Adoptado:** Usar directamente los casos de uso de cada crate.

**Razón:** 
- Evita duplicación de contratos
- Usa la API pública natural de cada bounded context
- Más mantenible y claro

---

### ✅ Decisión 2: PolicySet como Moneda de Intercambio
**Estándar:** Todos los casos de uso devuelven `cedar_policy::PolicySet`.

**Razón:**
- Interfaz común entre crates
- No expone entidades internas de dominio
- Directamente evaluable por `AuthorizationEngine`

---

### ✅ Decisión 3: Orquestación en Authorizer
**Responsabilidad:** `hodei-authorizer` orquesta, NO ejecuta.

**Patrón:**
```rust
// ✅ CORRECTO
let iam_policies = iam_use_case.execute(query).await?;
let scps = org_use_case.execute(query).await?;
let combined = combine(iam_policies, scps);
let decision = engine.is_authorized_with_policy_set(&request, &combined);

// ❌ INCORRECTO
let policies = self.load_policies_from_database().await?;
```

---

## 🚀 Plan de Implementación

### Sprint 1: Completar `hodei-iam` (1-2 días)
- [ ] Definir ports para repositorios
- [ ] Implementar adaptadores
- [ ] Completar lógica de `GetEffectivePoliciesForPrincipalUseCase`
- [ ] Tests unitarios

### Sprint 2: Integrar Entidades Reales (1 día)
- [ ] Crear `PrincipalResolver` y `ResourceResolver`
- [ ] Actualizar `EvaluatePermissionsUseCase`
- [ ] Eliminar `MockHodeiEntity`

### Sprint 3: Tests de Integración (1 día)
- [ ] Crear suite de tests end-to-end
- [ ] Validar casos de SCP Deny, IAM Allow, Implicit Deny
- [ ] Tests de performance

### Sprint 4: Optimizaciones (1 día)
- [ ] Implementar cache de políticas
- [ ] Añadir logging detallado
- [ ] Métricas de performance
- [ ] Documentación de API

---

## 📊 Checklist de Verificación

### Epic 2: Motor de Autorización ✅ (Parcialmente)
- [x] Crate `hodei-authorizer` existe
- [x] `EvaluatePermissionsUseCase` con orquestación
- [x] Integración con `AuthorizationEngine`
- [ ] `GetEffectivePoliciesForPrincipalUseCase` completado
- [ ] Entidades reales (no mocks)
- [ ] Tests de integración

### Epic 3: Integrar SCPs ✅ (Parcialmente)
- [x] `GetEffectiveScpsUseCase` implementado
- [x] Integrado en `hodei-authorizer`
- [x] Evaluación multi-capa
- [ ] Tests con SCPs + IAM

---

## 🎯 Resultado Esperado

**Estado Final:** Sistema de autorización multi-capa completamente funcional que:

1. ✅ Evalúa SCPs ANTES de políticas IAM
2. ✅ Combina políticas de múltiples fuentes
3. ✅ Usa Cedar Policy Engine para evaluación
4. ✅ Sigue principio de Least Privilege
5. ✅ Provee trazabilidad completa (determining_policies)
6. ✅ Cachea decisiones para performance
7. ✅ Registra todas las decisiones para auditoría

**API REST:**
```http
POST /api/v1/authorize
{
  "principal": "hrn:hodei:iam::user/alice",
  "action": "s3:GetObject",
  "resource": "hrn:hodei:s3:account-123:bucket/data",
  "context": {
    "source_ip": "192.168.1.1",
    "request_time": "2024-01-15T10:30:00Z"
  }
}

Response:
{
  "decision": "Allow",
  "determining_policies": ["iam-policy-123", "scp-456"],
  "reason": "Access explicitly allowed by policy",
  "explicit": true
}
```

---

## 📚 Referencias

- **Cedar Policy Language:** https://www.cedarpolicy.com/
- **AWS IAM Evaluation Logic:** https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_evaluation-logic.html
- **AWS SCPs:** https://docs.aws.amazon.com/organizations/latest/userguide/orgs_manage_policies_scps.html

---

**Última Actualización:** 2024-01-XX  
**Autor:** AI Development Agent  
**Estado:** 🟡 Implementación en progreso - 70% completado