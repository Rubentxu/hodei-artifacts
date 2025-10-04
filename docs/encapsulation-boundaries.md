# Encapsulación y Separación de Responsabilidades - Hodei Artifacts

**Fecha:** 2024  
**Estado:** ✅ IMPLEMENTADO Y VALIDADO

---

## 🎯 Principio Fundamental

> **"Cada crate expone SOLO su API pública a través de features y casos de uso.  
> Los detalles de implementación (como PolicyStorage, SurrealDB, etc.) son INTERNOS y NO se exponen."**

---

## 📦 Boundaries de Encapsulación por Crate

### 1. **policies** - Motor de Políticas Cedar

#### ✅ API Pública (LO QUE SE EXPONE)

```rust
// ✅ Estructuras públicas
pub struct AuthorizationEngine { ... }
pub struct AuthorizationRequest<'a> { ... }
pub struct EngineBuilder { ... }

// ✅ Métodos públicos
impl AuthorizationEngine {
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response
    pub fn is_authorized_with_policy_set(&self, request: &AuthorizationRequest<'_>, policies: &PolicySet) -> Response
}

// ✅ Traits públicos para extensibilidad
pub trait HodeiEntity { ... }
pub trait Principal { ... }
pub trait Resource { ... }
pub trait Action { ... }

// ✅ Features CRUD (casos de uso públicos)
pub mod features {
    pub mod create_policy;
    pub mod update_policy;
    pub mod delete_policy;
    pub mod get_policy;
    pub mod list_policies;
    // ... etc
}
```

#### ❌ Detalles Internos (NO SE EXPONEN)

```rust
// ❌ NO exponer PolicyStorage - es un detalle de implementación
// pub use domain::ports::PolicyStorage;  // <- INCORRECTO

// ❌ NO exponer PolicyStore - es interno
// pub use application::PolicyStore;  // <- INCORRECTO (solo dentro del crate)

// ❌ NO exponer implementaciones de storage
// pub use infrastructure::surreal::SurrealMemStorage;  // <- INCORRECTO
// pub use infrastructure::surreal::SurrealEmbeddedStorage;  // <- INCORRECTO
```

#### 📋 Exports Públicos Correctos

```rust
// En policies/src/shared/mod.rs
pub use application::{AuthorizationEngine, AuthorizationRequest, EngineBuilder};
pub use domain::{
    entity_utils,
    hrn::Hrn,
    ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource},
    // ❌ NO: PolicyStorage, StorageError
    schema_assembler::*,
};
pub use cedar_policy::{Context, EntityUid, Policy, PolicyId};
```

---

### 2. **hodei-iam** - Gestión de Identidades

#### ✅ API Pública

```rust
// ✅ Entidades de dominio públicas
pub struct User { ... }
pub struct Group { ... }
pub struct Role { ... }

// ✅ Features públicas (casos de uso)
pub mod features {
    pub mod create_user;
    pub mod attach_policy_to_user;
    pub mod list_user_policies;
    // ... etc
}

// ✅ Provider para hodei-authorizer
pub trait IamPolicyProvider: Send + Sync {
    async fn get_identity_policies_for(&self, principal: &Hrn) -> Result<PolicySet, Error>;
}
```

#### ❌ Detalles Internos

```rust
// ❌ NO exponer repositorios concretos
// pub use infrastructure::UserRepositoryImpl;  // <- INCORRECTO

// ❌ NO exponer detalles de persistencia
// pub use infrastructure::SurrealUserStorage;  // <- INCORRECTO

// ❌ Los adapters son internos a cada feature
// pub use features::create_user::adapter::UserPersisterAdapter;  // <- INCORRECTO
```

---

### 3. **hodei-organizations** - Estructura Organizacional

#### ✅ API Pública

```rust
// ✅ Entidades de dominio públicas
pub struct Organization { ... }
pub struct OrganizationalUnit { ... }
pub struct Account { ... }
pub struct ServiceControlPolicy { ... }

// ✅ Features públicas
pub mod features {
    pub mod create_organization;
    pub mod create_ou;
    pub mod attach_scp;
    pub mod get_effective_scps;
    // ... etc
}

// ✅ Provider para hodei-authorizer
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(&self, resource: &Hrn) -> Result<PolicySet, Error>;
}
```

#### ❌ Detalles Internos

```rust
// ❌ NO exponer implementaciones de repositorios
// ❌ NO exponer adapters de persistencia
// ❌ Los detalles de SurrealDB son internos
```

---

### 4. **hodei-authorizer** - Orquestador de Autorización

#### ✅ API Pública

```rust
// ✅ Use case principal
pub struct EvaluatePermissionsUseCase<IAM, ORG, CACHE, LOGGER, METRICS> { ... }

// ✅ DTOs públicos
pub struct AuthorizationRequest { ... }
pub struct AuthorizationResponse { ... }

// ✅ Traits para inyección de dependencias
pub trait IamPolicyProvider { ... }
pub trait OrganizationBoundaryProvider { ... }
pub trait AuthorizationCache { ... }
// ... etc
```

#### ❌ NO Debe Hacer

```rust
// ❌ NO construir AuthorizationEngine internamente
// let engine = AuthorizationEngine::new(...);  // <- INCORRECTO

// ❌ NO usar PolicyStorage directamente
// let storage = SurrealMemStorage::new(...);  // <- INCORRECTO

// ❌ NO gestionar PolicyStore
// let store = PolicyStore::new(...);  // <- INCORRECTO

// ✅ CORRECTO: Recibir el engine como dependencia inyectada
pub fn new(
    iam_provider: IAM,
    org_provider: ORG,
    authorization_engine: Arc<AuthorizationEngine>,  // <- ✅ Inyectado desde fuera
    // ...
) -> Self
```

---

## 🏗️ Responsabilidad de Construcción

### Quién Construye Qué

```rust
// En el APPLICATION LAYER (main.rs o similar)

async fn main() {
    // 1. ✅ La aplicación construye el AuthorizationEngine usando la API de policies
    let engine = create_authorization_engine().await;
    
    // 2. ✅ La aplicación construye los providers
    let iam_provider = create_iam_provider();
    let org_provider = create_org_provider();
    
    // 3. ✅ La aplicación inyecta todo en hodei-authorizer
    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Arc::new(engine),  // <- Construido externamente
        // ...
    );
}

async fn create_authorization_engine() -> AuthorizationEngine {
    // Aquí la aplicación decide:
    // - Qué storage usar (SurrealDB en memoria, persistente, etc.)
    // - Qué schema registrar
    // - Configuración específica
    
    // La aplicación PUEDE usar los detalles internos de policies si es necesario
    // porque está en el mismo workspace, pero hodei-authorizer NO
    use policies::infrastructure::surreal::SurrealMemStorage;
    
    let storage = SurrealMemStorage::new("app", "db").await.unwrap();
    
    let mut builder = policies::shared::EngineBuilder::new();
    builder.register_principal::<User>()?;
    builder.register_resource::<Bucket>()?;
    builder.register_action::<ReadAction>()?;
    
    let (engine, _store) = builder.build(Arc::new(storage))?;
    engine
}
```

---

## 🚫 Antipatrones a Evitar

### ❌ Antipatrón 1: Exponer Detalles de Implementación

```rust
// ❌ MAL: En policies/src/lib.rs
pub use infrastructure::surreal::SurrealMemStorage;
pub use domain::ports::PolicyStorage;

// Problema: Otros crates ahora dependen de detalles internos
// Si cambias la implementación de storage, rompes otros crates
```

### ❌ Antipatrón 2: Construir Dependencias Internas en Otros Crates

```rust
// ❌ MAL: En hodei-authorizer
let storage = SurrealMemStorage::new("test", "test").await?;
let store = PolicyStore::new(schema, storage);
let engine = AuthorizationEngine { schema, store };

// Problema: hodei-authorizer ahora conoce detalles internos de policies
// Si policies cambia su arquitectura interna, hodei-authorizer se rompe
```

### ❌ Antipatrón 3: Compartir PolicyStore Entre Crates

```rust
// ❌ MAL: Crear un PolicyStore global y compartirlo
static GLOBAL_POLICY_STORE: PolicyStore = ...;

// Problema: Acoplamiento global, difícil de testear, viola encapsulación
```

---

## ✅ Patrones Correctos

### ✅ Patrón 1: Inyección de Dependencias

```rust
// ✅ BIEN: hodei-authorizer recibe el engine ya construido
pub struct EvaluatePermissionsUseCase<...> {
    authorization_engine: Arc<AuthorizationEngine>,  // <- Inyectado
}

impl EvaluatePermissionsUseCase<...> {
    pub fn new(
        // ...
        authorization_engine: Arc<AuthorizationEngine>,
    ) -> Self {
        Self { authorization_engine, /* ... */ }
    }
}
```

### ✅ Patrón 2: API Pública Clara

```rust
// ✅ BIEN: policies expone métodos públicos, no estructuras internas
impl AuthorizationEngine {
    // API para políticas persistentes (usa PolicyStore interno)
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response
    
    // API para políticas dinámicas (sin necesitar PolicyStore)
    pub fn is_authorized_with_policy_set(&self, request: &AuthorizationRequest<'_>, policies: &PolicySet) -> Response
}
```

### ✅ Patrón 3: Providers de Políticas

```rust
// ✅ BIEN: Los providers devuelven PolicySet, no exponen internals
pub trait IamPolicyProvider {
    async fn get_identity_policies_for(&self, principal: &Hrn) -> Result<PolicySet, Error>;
}

// El orquestador combina PolicySets y delega
let iam_policies = iam_provider.get_identity_policies_for(principal).await?;
let scps = org_provider.get_effective_scps_for(resource).await?;

let mut combined = PolicySet::new();
for policy in iam_policies.policies() { combined.add(policy.clone())?; }
for policy in scps.policies() { combined.add(policy.clone())?; }

let response = engine.is_authorized_with_policy_set(&request, &combined);
```

---

## 🧪 Testing y Encapsulación

### Tests Unitarios

```rust
// ✅ Tests de hodei-authorizer NO deben construir el engine

#[cfg(test)]
mod tests {
    // ⚠️ Si necesitas un engine para tests, usa una función helper
    // claramente marcada como "SOLO PARA TESTS"
    
    /// Helper para tests del DI container ÚNICAMENTE
    /// En producción, el engine se construye en main.rs
    fn create_test_engine_for_di_tests() -> AuthorizationEngine {
        // Construcción mínima para validar wiring del DI
        // NO es código de producción
    }
    
    #[test]
    fn test_di_container_wiring() {
        let engine = create_test_engine_for_di_tests();  // Solo para el test
        let use_case = EvaluatePermissionsUseCase::new(
            // ...
            Arc::new(engine),
        );
        // Validar que el DI funciona
    }
}
```

### Tests de Integración

```rust
// ✅ Tests E2E construyen el sistema completo
// Esto está BIEN porque simula el comportamiento real de la aplicación

#[tokio::test]
async fn test_complete_authorization_flow() {
    // Construir engine (como lo haría main.rs)
    let engine = setup_real_engine().await;
    
    // Construir providers reales
    let iam_provider = setup_iam_provider().await;
    let org_provider = setup_org_provider().await;
    
    // Inyectar en el use case
    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Arc::new(engine),
        // ...
    );
    
    // Ejecutar test E2E
    let response = use_case.execute(request).await.unwrap();
    assert!(response.decision == AuthorizationDecision::Allow);
}
```

---

## 📊 Resumen de Encapsulación

| Crate | Expone | NO Expone |
|-------|--------|-----------|
| **policies** | `AuthorizationEngine`, `EngineBuilder`, Features CRUD | `PolicyStorage`, `PolicyStore`, `SurrealMemStorage` |
| **hodei-iam** | Entidades, Features, `IamPolicyProvider` | Repositorios concretos, Adapters, Detalles de persistencia |
| **hodei-organizations** | Entidades, Features, `OrganizationBoundaryProvider` | Repositorios concretos, Adapters, Detalles de persistencia |
| **hodei-authorizer** | `EvaluatePermissionsUseCase`, DTOs, Traits | NO construye `AuthorizationEngine`, NO usa `PolicyStorage` |

---

## ✅ Criterios de Validación

Para verificar que la encapsulación es correcta:

1. ✅ **hodei-authorizer NO importa tipos internos:**
   ```bash
   # Este comando NO debe encontrar matches
   grep -r "PolicyStorage\|PolicyStore\|SurrealMemStorage" hodei-artifacts/crates/hodei-authorizer/src/
   ```

2. ✅ **policies NO expone detalles internos públicamente:**
   ```rust
   // En policies/src/shared/mod.rs, NO debe haber:
   // pub use infrastructure::surreal::*;
   // pub use domain::ports::PolicyStorage;
   ```

3. ✅ **AuthorizationEngine se inyecta, no se construye:**
   ```rust
   // hodei-authorizer recibe Arc<AuthorizationEngine>
   // NO lo construye internamente
   ```

---

## 🎓 Principios Aplicados

1. **Information Hiding:** Los detalles de implementación están ocultos
2. **Dependency Inversion:** Dependencia de abstracciones, no de concreciones
3. **Single Responsibility:** Cada crate tiene una responsabilidad clara
4. **Open/Closed:** Abierto a extensión (traits), cerrado a modificación (internals)
5. **Separation of Concerns:** Construcción separada de uso

---

**Conclusión:** La encapsulación correcta permite que cada crate evolucione independientemente sin romper otros crates. Solo se expone lo necesario a través de APIs públicas bien definidas.

---

**Autor:** Sistema Hodei  
**Versión:** 1.0  
**Última actualización:** 2024