# Plan de Refactorización: Principio de Responsabilidad Única

**Fecha:** 2024  
**Objetivo:** Asegurar que cada crate expone SOLO su API a través de casos de uso (features)

---

## 🎯 Principio Fundamental

> **"Un crate SOLO expone casos de uso (features) con Commands/Queries/DTOs.  
> Las entidades de dominio, repositorios y servicios son INTERNOS y NUNCA se exponen."**

---

## ❌ Problemas Actuales Identificados

### 1. **hodei-iam** - Exponiendo Entidades de Dominio

**Problema:**
```rust
// ❌ MAL: En hodei-iam/src/lib.rs
pub use shared::domain::{
    CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User,
};
```

**Impacto:**
- Otros crates pueden importar `User`, `Group` directamente
- Viola encapsulación
- Crea acoplamiento entre crates
- Si cambia la entidad `User`, todos los crates que la usan se rompen

---

### 2. **hodei-organizations** - Exponiendo Entidades de Dominio

**Problema:**
```rust
// ❌ MAL: En hodei-organizations/src/shared/domain/mod.rs
pub use account::Account;
pub use ou::OrganizationalUnit;
pub use scp::ServiceControlPolicy;
```

**Impacto:**
- `hodei-authorizer` importa `ServiceControlPolicy` directamente
- Viola principio de responsabilidad única
- Las entidades deben ser internas

---

### 3. **hodei-authorizer** - Usando Entidades de Otros Crates

**Problema:**
```rust
// ❌ MAL: En hodei-authorizer/src/ports.rs
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;
```

**Impacto:**
- Acceso directo a entidades internas de otro crate
- Debería usar solo casos de uso

---

### 4. **Casos de Uso Devolviendo Entidades**

**Problema:**
```rust
// ❌ MAL: En get_effective_scps/use_case.rs
pub async fn execute(&self, target_hrn: String) 
    -> Result<Vec<ServiceControlPolicy>, Error>  // <- Devuelve entidad interna
```

**Impacto:**
- El caso de uso expone entidades internas
- Debería devolver DTOs o PolicySet de Cedar

---

## ✅ Arquitectura Correcta

### Flujo de Comunicación Entre Crates

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                         │
│                          (main.rs)                               │
│                                                                   │
│  - Construye casos de uso                                        │
│  - Inyecta dependencias                                          │
│  - Orquesta llamadas entre crates                                │
└─────────────────────────────────────────────────────────────────┘
         │                    │                     │
         ▼                    ▼                     ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────────┐
│  hodei-iam   │    │ hodei-org    │    │ hodei-authorizer │
│              │    │              │    │                  │
│ ✅ Expone:   │    │ ✅ Expone:   │    │ ✅ Expone:       │
│ - UseCases   │    │ - UseCases   │    │ - UseCases       │
│ - Commands   │    │ - Commands   │    │ - Commands       │
│ - Queries    │    │ - Queries    │    │ - Queries        │
│ - DTOs       │    │ - DTOs       │    │ - DTOs           │
│              │    │              │    │                  │
│ ❌ NO Expone:│    │ ❌ NO Expone:│    │ ❌ NO Expone:    │
│ - User       │    │ - Account    │    │ - Nada interno   │
│ - Group      │    │ - OU         │    │                  │
│ - Repos      │    │ - SCP        │    │                  │
│              │    │ - Repos      │    │                  │
└──────────────┘    └──────────────┘    └──────────────────┘
```

---

## 📋 Plan de Refactorización

### Fase 1: hodei-iam ✅

#### 1.1. Eliminar Exports de Entidades

**Cambio:**
```rust
// ❌ Eliminar esto:
pub use shared::domain::{
    CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User,
};

// ✅ Solo exportar casos de uso:
pub use features::{
    add_user_to_group::AddUserToGroupUseCase,
    create_group::CreateGroupUseCase,
    create_user::CreateUserUseCase,
};
```

#### 1.2. Crear Caso de Uso para hodei-authorizer

**Nuevo Feature:**
```rust
// hodei-iam/src/features/get_effective_policies_for_principal/

// dto.rs
pub struct GetEffectivePoliciesQuery {
    pub principal_hrn: String,
}

pub struct EffectivePoliciesResponse {
    pub policies: PolicySet,  // Cedar PolicySet, no entidades internas
}

// use_case.rs
pub struct GetEffectivePoliciesForPrincipalUseCase<UR, GR, PR> {
    user_repo: UR,
    group_repo: GR,
    policy_repo: PR,
}

impl GetEffectivePoliciesForPrincipalUseCase {
    pub async fn execute(&self, query: GetEffectivePoliciesQuery) 
        -> Result<EffectivePoliciesResponse, Error> 
    {
        // 1. Resolver el principal (usuario)
        let user = self.user_repo.find_by_hrn(&query.principal_hrn).await?;
        
        // 2. Obtener grupos del usuario
        let groups = self.group_repo.find_by_user_hrn(&user.hrn).await?;
        
        // 3. Recolectar políticas directas del usuario
        let user_policies = self.policy_repo.find_by_user_hrn(&user.hrn).await?;
        
        // 4. Recolectar políticas de los grupos
        let mut group_policies = Vec::new();
        for group in groups {
            let policies = self.policy_repo.find_by_group_hrn(&group.hrn).await?;
            group_policies.extend(policies);
        }
        
        // 5. Combinar en PolicySet de Cedar
        let mut policy_set = PolicySet::new();
        for policy in user_policies.into_iter().chain(group_policies) {
            policy_set.add(policy.to_cedar_policy())?;
        }
        
        Ok(EffectivePoliciesResponse { policies: policy_set })
    }
}
```

**Export:**
```rust
// hodei-iam/src/lib.rs
pub use features::get_effective_policies_for_principal::{
    GetEffectivePoliciesForPrincipalUseCase,
    GetEffectivePoliciesQuery,
    EffectivePoliciesResponse,
};
```

---

### Fase 2: hodei-organizations ✅

#### 2.1. Hacer Entidades Internas

**Cambio:**
```rust
// hodei-organizations/src/shared/domain/mod.rs

// ❌ Cambiar de pub a pub(crate)
pub(crate) mod account;
pub(crate) mod ou;
pub(crate) mod scp;

pub(crate) use account::Account;
pub(crate) use ou::OrganizationalUnit;
pub(crate) use scp::ServiceControlPolicy;
```

#### 2.2. Actualizar get_effective_scps para devolver PolicySet

**Cambio:**
```rust
// hodei-organizations/src/features/get_effective_scps/dto.rs

pub struct GetEffectiveScpsQuery {
    pub resource_hrn: String,
}

pub struct EffectiveScpsResponse {
    pub policies: PolicySet,  // Cedar PolicySet, no entidades
}

// use_case.rs
impl GetEffectiveScpsUseCase {
    pub async fn execute(&self, query: GetEffectiveScpsQuery) 
        -> Result<EffectiveScpsResponse, Error> 
    {
        // Lógica interna usa ServiceControlPolicy (entidad)
        let scps = self.collect_effective_scps(&query.resource_hrn).await?;
        
        // Convertir a PolicySet de Cedar
        let mut policy_set = PolicySet::new();
        for scp in scps {
            policy_set.add(scp.to_cedar_policy())?;
        }
        
        Ok(EffectiveScpsResponse { policies: policy_set })
    }
}
```

#### 2.3. Actualizar Exports

```rust
// hodei-organizations/src/lib.rs

// ✅ Solo exportar casos de uso
pub use features::{
    attach_scp::AttachScpUseCase,
    create_account::CreateAccountUseCase,
    create_ou::CreateOuUseCase,
    get_effective_scps::{
        GetEffectiveScpsUseCase,
        GetEffectiveScpsQuery,
        EffectiveScpsResponse,
    },
};
```

---

### Fase 3: hodei-authorizer ✅

#### 3.1. Eliminar Providers, Usar Casos de Uso Directamente

**Cambio:**
```rust
// hodei-authorizer/src/features/evaluate_permissions/use_case.rs

pub struct EvaluatePermissionsUseCase {
    // ✅ Casos de uso de otros crates, NO providers custom
    iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,
    org_use_case: Arc<GetEffectiveScpsUseCase>,
    authorization_engine: Arc<AuthorizationEngine>,
    cache: Option<Arc<dyn AuthorizationCache>>,
    logger: Arc<dyn AuthorizationLogger>,
    metrics: Arc<dyn AuthorizationMetrics>,
}

impl EvaluatePermissionsUseCase {
    pub async fn execute(&self, request: AuthorizationRequest) 
        -> Result<AuthorizationResponse, Error> 
    {
        // 1. Obtener políticas IAM usando caso de uso de hodei-iam
        let iam_query = GetEffectivePoliciesQuery {
            principal_hrn: request.principal.to_string(),
        };
        let iam_response = self.iam_use_case.execute(iam_query).await?;
        
        // 2. Obtener SCPs usando caso de uso de hodei-organizations
        let scp_query = GetEffectiveScpsQuery {
            resource_hrn: request.resource.to_string(),
        };
        let scp_response = self.org_use_case.execute(scp_query).await?;
        
        // 3. Combinar PolicySets
        let mut combined = PolicySet::new();
        for policy in iam_response.policies.policies() {
            combined.add(policy.clone())?;
        }
        for policy in scp_response.policies.policies() {
            combined.add(policy.clone())?;
        }
        
        // 4. Delegar evaluación a policies engine
        let response = self.authorization_engine
            .is_authorized_with_policy_set(&cedar_request, &combined);
        
        Ok(AuthorizationResponse::from(response))
    }
}
```

#### 3.2. Eliminar ports.rs con IamPolicyProvider/OrganizationBoundaryProvider

**Acción:** Eliminar archivo `ports.rs` ya que no necesitamos providers custom.

#### 3.3. Actualizar DI Container

```rust
// hodei-authorizer/src/features/evaluate_permissions/di.rs

pub struct EvaluatePermissionsContainer {
    iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,
    org_use_case: Arc<GetEffectiveScpsUseCase>,
    authorization_engine: Arc<AuthorizationEngine>,
    cache: Option<Arc<dyn AuthorizationCache>>,
    logger: Arc<dyn AuthorizationLogger>,
    metrics: Arc<dyn AuthorizationMetrics>,
}
```

---

### Fase 4: Application Layer (main.rs) ✅

```rust
// src/main.rs

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Construir casos de uso de hodei-iam
    let iam_use_case = build_iam_use_case().await?;
    
    // 2. Construir casos de uso de hodei-organizations
    let org_use_case = build_org_use_case().await?;
    
    // 3. Construir AuthorizationEngine
    let auth_engine = build_authorization_engine().await?;
    
    // 4. Construir hodei-authorizer use case inyectando dependencias
    let evaluate_permissions = EvaluatePermissionsUseCase::new(
        Arc::new(iam_use_case),
        Arc::new(org_use_case),
        Arc::new(auth_engine),
        // ... cache, logger, metrics
    );
    
    // 5. Usar el caso de uso
    let response = evaluate_permissions.execute(request).await?;
    
    Ok(())
}

async fn build_iam_use_case() -> GetEffectivePoliciesForPrincipalUseCase {
    // Construir repositorios
    let user_repo = build_user_repo().await;
    let group_repo = build_group_repo().await;
    let policy_repo = build_policy_repo().await;
    
    GetEffectivePoliciesForPrincipalUseCase::new(
        user_repo,
        group_repo,
        policy_repo,
    )
}

async fn build_org_use_case() -> GetEffectiveScpsUseCase {
    // Construir repositorios
    let scp_repo = build_scp_repo().await;
    let org_repo = build_org_repo().await;
    
    GetEffectiveScpsUseCase::new(scp_repo, org_repo)
}
```

---

## 🔍 Validación de Arquitectura Correcta

### Checklist de Verificación

#### ✅ hodei-iam
- [ ] NO exporta entidades (`User`, `Group`, etc.)
- [ ] Solo exporta casos de uso (features)
- [ ] Casos de uso aceptan Commands/Queries
- [ ] Casos de uso devuelven DTOs (no entidades)
- [ ] Tiene feature `GetEffectivePoliciesForPrincipal`

#### ✅ hodei-organizations
- [ ] NO exporta entidades (`Account`, `OU`, `ServiceControlPolicy`)
- [ ] Solo exporta casos de uso (features)
- [ ] `GetEffectiveScps` devuelve `PolicySet` (no entidades)
- [ ] Entidades son `pub(crate)` (internas)

#### ✅ hodei-authorizer
- [ ] NO importa entidades de otros crates
- [ ] USA casos de uso de otros crates directamente
- [ ] NO tiene providers custom (IamPolicyProvider, etc.)
- [ ] Inyecta casos de uso, no repositorios

#### ✅ policies
- [ ] NO exporta `PolicyStorage`, `PolicyStore`
- [ ] Solo exporta `AuthorizationEngine` y casos de uso
- [ ] Detalles de SurrealDB son internos

---

## 📊 Comparación: Antes vs Después

| Aspecto | ❌ Antes (Incorrecto) | ✅ Después (Correcto) |
|---------|----------------------|----------------------|
| **Exports** | Entidades de dominio | Solo casos de uso |
| **Comunicación** | Importar entidades directamente | Llamar casos de uso |
| **Acoplamiento** | Alto (depende de entidades) | Bajo (depende de DTOs) |
| **Encapsulación** | Rota (entidades públicas) | Respetada (entidades internas) |
| **Testabilidad** | Difícil (acoplamiento) | Fácil (casos de uso mockeables) |
| **Mantenibilidad** | Difícil (cambios rompen otros crates) | Fácil (cambios internos no afectan) |

---

## 🚀 Beneficios de la Arquitectura Correcta

### 1. **Encapsulación Perfecta**
- Cada crate puede cambiar sus entidades internas sin afectar a otros
- Los detalles de implementación están ocultos

### 2. **Bajo Acoplamiento**
- Los crates solo conocen los casos de uso de otros crates
- No hay dependencia de estructuras internas

### 3. **Alta Cohesión**
- Cada caso de uso tiene una responsabilidad única y clara
- Los casos de uso agrupan lógica relacionada

### 4. **Fácil Testing**
- Los casos de uso se mockean fácilmente
- No necesitas mockear entidades complejas

### 5. **Evolutivo**
- Agregar nuevas funcionalidades = agregar nuevos casos de uso
- Modificar funcionalidades = modificar casos de uso aislados

---

## 📝 Reglas de Oro

### ✅ HACER:
1. Exportar solo casos de uso (features) desde lib.rs
2. Casos de uso aceptan Commands/Queries/DTOs
3. Casos de uso devuelven DTOs/Events/Responses
4. Marcar entidades como `pub(crate)` (internas al crate)
5. Usar casos de uso de otros crates directamente (no crear wrappers)

### ❌ NO HACER:
1. Exportar entidades de dominio desde lib.rs
2. Exportar repositorios o services
3. Importar entidades de otros crates
4. Crear providers que wrappean casos de uso de otros crates
5. Devolver entidades desde casos de uso

---

## 🎓 Principios Aplicados

1. **Single Responsibility Principle (SRP)**
   - Cada crate tiene UNA forma de interacción: casos de uso

2. **Interface Segregation Principle (ISP)**
   - Solo se expone lo necesario (casos de uso), no todo

3. **Dependency Inversion Principle (DIP)**
   - Dependencia de abstracciones (casos de uso) no de concreciones (entidades)

4. **Open/Closed Principle (OCP)**
   - Abierto a extensión (nuevos casos de uso)
   - Cerrado a modificación (entidades internas)

5. **Information Hiding**
   - Detalles internos (entidades, repos) están ocultos

---

**Conclusión:** Esta refactorización asegura que cada crate respeta el principio de responsabilidad única, exponiendo SOLO su API pública a través de casos de uso bien definidos con Commands/Queries/DTOs, manteniendo todas las entidades de dominio y servicios como detalles internos de implementación.

---

**Autor:** Sistema Hodei  
**Versión:** 1.0  
**Estado:** Plan de Refactorización  
**Próximo Paso:** Implementar Fase 1 (hodei-iam)