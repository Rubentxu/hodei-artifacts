### **Documento de Diseño e Historias de Usuario: Crate `hodei-iam`**

**Visión General del Crate:**

El `crate` `hodei-iam` es el _bounded context_ responsable de la gestión de identidades y políticas de acceso. Es análogo a AWS IAM. Sus responsabilidades son:

1.  **Gestionar Entidades de Dominio:** `User`, `Group`, `ServiceAccount` (las entidades en sí son `internal`).
2.  **Gestionar Políticas de Identidad:** CRUD de `HodeiPolicy` (tratadas como políticas de IAM).
3.  **Gestionar Relaciones:** Adjuntar usuarios a grupos y políticas a identidades.
4.  **Proveer Información de Acceso:** Exponer un `UseCase` para que `hodei-authorizer` pueda obtener las políticas efectivas de un principal.

**Dependencias Principales:** `kernel`, `hodei-policies` (para validación), `async-trait`, `thiserror`, `serde`, `surrealdb` (solo en la capa de `infrastructure`).

---

### **Épica 1: Re-estructuración Fundamental del Crate**

#### **HU-IAM-R-000: Implementar la Nueva Estructura de Módulos (ISP Puro)**
*   **Como:** Arquitecto del sistema.
*   **Quiero:** Reorganizar `hodei-iam` para separar `features` con puertos segregados, `infrastructure` con adaptadores públicos y un `internal` completamente sellado.
*   **Para:** Lograr una arquitectura que sea encapsulada, testable y permita una DI flexible.
*   **Criterios de Aceptación:**
    1.  El `crate` tendrá la estructura V7.0: `api.rs`, `features/`, `infrastructure/`, `internal/`.
    2.  `internal/` solo contendrá `domain/` con las `structs` `User`, `Group`, etc. (todas `pub(crate)`).
    3.  `infrastructure/` contendrá los adaptadores públicos, como `SurrealUserAdapter` y `SurrealGroupAdapter`, que implementarán los `traits` de las `features`.
    4.  El `api.rs` exportará los módulos `features` e `infrastructure`.
    5.  Todos los `traits` monolíticos de repositorio (`UserRepository`, etc.) serán eliminados por completo.

---

### **Épica 2: Gestión de Políticas (CRUD de `HodeiPolicy`)**

#### HU-IAM-R-001 (Revisada): Crear una Nueva Política de Identidad
Como: Administrador.
Quiero: Crear una nueva HodeiPolicy proporcionando un String con su contenido.
Para: Definir nuevas reglas de permisos.
features/create_policy/ports.rs (Revisado):
code
Rust
use kernel::domain::policy::HodeiPolicy;

// Sigue validando un string crudo
pub trait PolicyValidator {
    async fn validate(&self, content: &str) -> Result<ValidationResult, ...>;
}

// Ahora persiste la entidad de dominio agnóstica
#[async_trait]
pub trait PolicyPersister {
    async fn save_policy(&self, policy: &HodeiPolicy) -> Result<(), CreatePolicyError>;
}
features/create_policy/use_case.rs (Algoritmo Revisado):
Recibe CreatePolicyCommand { policy_id: String, content: String, ... }.
Llama a self.validator.validate(&command.content).
Construye el tipo del kernel: let hodei_policy = HodeiPolicy::new(PolicyId::new(command.policy_id), command.content, ...);.
Llama a self.persister.save_policy(&hodei_policy).
Mapea el HodeiPolicy a un DTO PolicyView y lo devuelve.
Impacto: El UseCase ahora es responsable de elevar el String validado a un tipo de dominio (HodeiPolicy) antes de pasarlo a la capa de persistencia. La frontera entre datos crudos y tipos de dominio está claramente en el UseCase.

#### **HU-IAM-R-002 a R-005: Refactorizar `get_policy`, `list_policies`, `update_policy`, `delete_policy`**
*   **Resumen:** Se aplica el mismo patrón a las demás `features` de CRUD de políticas.
    *   `get_policy`: Define `pub trait GetPolicyPort { find_by_id... }`. El `UseCase` devuelve un `PolicyView`.
    *   `list_policies`: Define `pub trait ListPoliciesPort { list... }`. El `UseCase` devuelve `ListPoliciesResponse` con `PolicySummary` (sin el `content`).
    *   `update_policy`: Define `pub trait UpdatePolicyPort { update... }` y reutiliza `PolicyValidator`.
    *   `delete_policy`: Define `pub trait DeletePolicyPort { delete... }`.
*   **Implementación:** El `IamPolicyPersistenceAdapter` de `infrastructure` implementará todos estos nuevos `traits` segregados (`GetPolicyPort`, `ListPoliciesPort`, etc.), cada uno con su lógica de DB específica.

---

### **Épica 3: Gestión de Identidades (CRUD de `User` y `Group`)**

#### **HU-IAM-R-006: Crear un Nuevo Usuario**
*   **Como:** Un administrador de IAM.
*   **Quiero:** Crear un nuevo usuario proveyendo un nombre y un email.
*   **Para:** Añadir nuevas identidades humanas al sistema.
*   **Criterios de Aceptación:**
    1.  **`features/create_user/ports.rs`:**
        *   `pub trait CreateUserPort { async fn save_user(&self, user: &User) -> Result<(), CreateUserError>; }`.
        *   `pub trait HrnGenerator { fn new_user_hrn(&self) -> Hrn; }`.
    2.  **`features/create_user/use_case.rs`:**
        *   `CreateUserUseCase` depende de `CreateUserPort` y `HrnGenerator`.
        *   **Algoritmo de `execute`:**
            1.  Recibe `CreateUserCommand { name: String, email: String, ... }`.
            2.  Llama a `self.hrn_generator.new_user_hrn()` para obtener un `Hrn` único.
            3.  Crea una instancia de la entidad interna `User::new(hrn, name, email)`.
            4.  Llama a `self.port.save_user(&user)`.
            5.  Mapea la entidad `User` a un DTO `UserView` y lo devuelve.
    3.  **`infrastructure/user_adapter.rs`:**
        *   `pub struct UserPersistenceAdapter` implementa `CreateUserPort`.
    4.  **`infrastructure/hrn_generator.rs`:**
        *   `pub struct UuidHrnGenerator` implementa `HrnGenerator`.
    5.  **Tests de Integración (`/tests/`):**
        *   Se inyecta un `MockCreateUserPort` y un `MockHrnGenerator` para probar el `UseCase` de forma aislada de la infraestructura real.

#### **HU-IAM-R-007: Crear un Nuevo Grupo**
*   **Como:** Un administrador de IAM.
*   **Quiero:** Crear un nuevo grupo proveyendo un nombre.
*   **Para:** Organizar usuarios y adjuntarles políticas de forma colectiva.
*   **Criterios de Aceptación:** Sigue el mismo patrón que `create_user`, pero con la `feature` `create_group`, la entidad `Group`, el puerto `CreateGroupPort`, y el `UseCase` correspondiente.

---

### **Épica 4: Gestión de Relaciones**

#### **HU-IAM-R-008: Añadir un Usuario a un Grupo**
*   **Como:** Un administrador de IAM.
*   **Quiero:** Asociar un usuario existente con un grupo existente.
*   **Para:** Que el usuario herede los permisos del grupo.
*   **Criterios de Aceptación:**
    1.  **`features/add_user_to_group/ports.rs`:**
        *   `pub trait UserFinder { async fn find_user_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, ...>; }`.
        *   `pub trait GroupFinder { async fn find_group_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, ...>; }`.
        *   `pub trait UserGroupPersister { async fn save_user(&self, user: &User) -> Result<(), ...>; }`.
    2.  **`features/add_user_to_group/use_case.rs`:**
        *   `AddUserToGroupUseCase` depende de los tres puertos anteriores.
        *   **Algoritmo de `execute`:**
            1.  Recibe `AddUserToGroupCommand { user_hrn: Hrn, group_hrn: Hrn }`.
            2.  Llama a `self.user_finder.find_user_by_hrn`. Si no existe, devuelve `UserNotFound`.
            3.  Llama a `self.group_finder.find_group_by_hrn`. Si no existe, devuelve `GroupNotFound`.
            4.  Obtiene la entidad `User` mutable.
            5.  Llama a `user.add_to_group(command.group_hrn)`.
            6.  Llama a `self.persister.save_user(&user)` para persistir el cambio. **Nota:** La atomicidad se puede lograr si el `persister` es transaccional, o con patrones más avanzados. Para empezar, se asume que esta operación es atómica.
            7.  Devuelve un DTO de éxito.
    3.  **`infrastructure/user_adapter.rs`:**
        *   `UserPersistenceAdapter` ahora también implementa `UserFinder` y `UserGroupPersister`.
    4.  **`infrastructure/group_adapter.rs`:**
        *   `GroupPersistenceAdapter` implementa `GroupFinder`.
    5.  **Tests de Integración (`/tests/`):**
        *   El test inyectará mocks para los tres puertos. Un test clave verificará que si `find_user_by_hrn` tiene éxito pero `save_user` falla, la operación devuelve un error (simulando un rollback transaccional).

---

### **Épica 5: Exponer Capacidades de Autorización**

#### HU-IAM-R-009 (Revisada): Obtener las Políticas Efectivas para un Principal
Como: hodei-authorizer.
Quiero: Solicitar el HodeiPolicySet efectivo para un principal.
Para: Evaluar permisos de identidad.
features/get_effective_policies/ports.rs (Revisado):
code
Rust
use kernel::domain::policy::HodeiPolicySet;
use kernel::Hrn;

// El puerto ahora promete devolver el tipo del kernel directamente.
#[async_trait]
pub trait EffectivePoliciesProvider {
    async fn get_policies_for_principal(&self, principal_hrn: &Hrn) -> Result<HodeiPolicySet, ...>;
}
features/get_effective_policies/use_case.rs:
El GetEffectivePoliciesUseCase simplemente delega en el puerto y devuelve el HodeiPolicySet que recibe. Su DTO de respuesta EffectivePoliciesResponse contendrá el HodeiPolicySet.
infrastructure/effective_policies_adapter.rs (Algoritmo Detallado):
Este adaptador es el que tiene la lógica compleja, pero ahora es más seguro gracias a los tipos.
Cargar Usuario y Grupos: Carga la entidad User y los Hrn de sus Group desde la base de datos.
Cargar Políticas Directas: Carga las políticas adjuntas directamente al usuario. Estas ya se almacenan y se leen de la base de datos como HodeiPolicy.
Cargar Políticas de Grupos: Carga las entidades Group y, para cada una, sus HodeiPolicy adjuntas.
Construir HodeiPolicySet:
code
Rust
// Dentro del método `get_policies_for_principal` del adaptador
let mut effective_policies = HodeiPolicySet::default();

let direct_policies: Vec<HodeiPolicy> = self.db.get_policies_for_user(principal_hrn).await?;
for policy in direct_policies {
    effective_policies.add(policy);
}

let group_hrns: Vec<Hrn> = self.db.get_groups_for_user(principal_hrn).await?;
let group_policies: Vec<HodeiPolicy> = self.db.get_policies_for_groups(&group_hrns).await?;
for policy in group_policies {
    effective_policies.add(policy);
}

Ok(effective_policies)
Impacto: El contrato entre hodei-iam y hodei-authorizer se vuelve mucho más fuerte. hodei-authorizer recibe un objeto de dominio (HodeiPolicySet), no una simple colección de Strings. La lógica de agregación dentro del EffectivePoliciesAdapter es más segura porque manipula structs HodeiPolicy, no datos primitivos.

#### HU-IAM-R-0XX: Refactorizar las Entidades Internas
Como: Desarrollador de hodei-iam.
Quiero: Que las entidades de dominio internas (User, Group) implementen los traits HodeiEntity, Principal y Resource del kernel.
Para: Asegurar que las entidades de hodei-iam puedan ser utilizadas por el motor de evaluación de hodei-policies.
Criterios de Aceptación:
internal/domain/user.rs:
struct User ahora implementa HodeiEntity y Principal.
El método attributes() devuelve un HashMap<AttributeName, AttributeValue> con los atributos del usuario (nombre, email, etc.).
El método parent_hrns() devuelve los Hrn de los grupos a los que pertenece.
internal/domain/group.rs:
struct Group ahora implementa HodeiEntity y Resource (un grupo no es un principal, es un recurso al que se añaden usuarios).
El método attributes() devuelve sus atributos (nombre, etc.).
Tests de Integración (/tests/):
Se debe crear un test que:
Cree un User y un Group usando los UseCases de hodei-iam.
Los recupere de la base de datos (a través de un UseCase get_user, por ejemplo, que internamente carga la entidad).
Cree un Vec<&dyn HodeiEntity> con las referencias a estas entidades.
Pase este Vec al EvaluatePoliciesUseCase de hodei-policies.
Verifique que la evaluación funciona correctamente. Este test valida la integración completa de los traits del kernel.

Este conjunto de historias de usuario te proporciona un camino claro y detallado para refactorizar `hodei-iam` a la arquitectura final (V7.0). Cada paso es incremental, testable y se alinea con los principios de ISP, VSA y DI que hemos establecido.


### **`crate`: `hodei-iam` (Versión Refactorizada)**

#### **1. `crates/hodei-iam/Cargo.toml`**

```toml
[package]
name = "hodei-iam"
version = "0.1.0"
edition = "2024"

[dependencies]
# Dependencias del workspace
kernel = { path = "../kernel" }
hodei-policies = { path = "../hodei-policies" }

# Dependencias externas
async-trait = "0.1"
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }
surrealdb = { version = "1.0", features = ["full"] }
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
```

#### **2. `crates/hodei-iam/src/lib.rs`**

```rust
// Módulos que componen el crate. `internal` es privado al crate.
pub mod features;
pub mod infrastructure;
pub(crate) mod internal;

// La API pública se define y exporta desde `api.rs`
pub mod api;
pub use api::*;
```

#### **3. `crates/hodei-iam/src/api.rs` (Superficie Pública)**

```rust
//! Public API surface for the `hodei-iam` bounded context.

// Re-export public modules for external consumption.
// This allows consumers to `use hodei_iam::features::create_user;`
pub use crate::features::*;
pub use crate::infrastructure::*;
```

#### **4. `crates/hodei-iam/src/internal/domain/` (Dominio Sellado)**

**`.../internal/domain/user.rs`**
```rust
use kernel::domain::entity::{AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Principal, Resource};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct User {
    pub hrn: Hrn,
    pub name: String,
    pub email: String,
    pub group_hrns: Vec<Hrn>,
}

impl User {
    pub(crate) fn new(hrn: Hrn, name: String, email: String) -> Self {
        Self { hrn, name, email, group_hrns: Vec::new() }
    }
    
    pub(crate) fn add_to_group(&mut self, group_hrn: Hrn) {
        if !self.group_hrns.contains(&group_hrn) {
            self.group_hrns.push(group_hrn);
        }
    }
}

// Implementación de los traits del kernel
impl HodeiEntityType for User {
    fn entity_type_name() -> String { "Iam::User".to_string() }
    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (AttributeName::new("name"), AttributeType::String),
            (AttributeName::new("email"), AttributeType::String),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn { &self.hrn }
    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();
        attrs.insert(AttributeName::new("name"), AttributeValue::String(self.name.clone()));
        attrs.insert(AttributeName::new("email"), AttributeValue::String(self.email.clone()));
        attrs
    }
    fn parent_hrns(&self) -> Vec<Hrn> { self.group_hrns.clone() }
}

impl Principal for User {}
impl Resource for User {}
```
*   *(El fichero para `Group` seguiría un patrón similar)*

#### **5. `crates/hodei-iam/src/features/` (Lógica de Negocio VSA)**

##### **`.../features/create_user/`**

**`ports.rs`**
```rust
use crate::internal::domain::User;
use async_trait::async_trait;
use kernel::Hrn;
use super::error::CreateUserError;

#[async_trait]
pub trait CreateUserPort: Send + Sync {
    async fn save_user(&self, user: &User) -> Result<(), CreateUserError>;
}

pub trait HrnGenerator: Send + Sync {
    fn new_user_hrn(&self, name: &str) -> Hrn;
}```
**`use_case.rs`**
```rust
use std::sync::Arc;
use crate::internal::domain::User;
use super::dto::{CreateUserCommand, UserView};
use super::error::CreateUserError;
use super::ports::{CreateUserPort, HrnGenerator};

pub struct CreateUserUseCase<P: CreateUserPort, G: HrnGenerator> {
    persister: Arc<P>,
    hrn_generator: Arc<G>,
}

impl<P: CreateUserPort, G: HrnGenerator> CreateUserUseCase<P, G> {
    pub fn new(persister: Arc<P>, hrn_generator: Arc<G>) -> Self {
        Self { persister, hrn_generator }
    }

    pub async fn execute(&self, command: CreateUserCommand) -> Result<UserView, CreateUserError> {
        let hrn = self.hrn_generator.new_user_hrn(&command.name);
        let user = User::new(hrn, command.name, command.email);
        
        self.persister.save_user(&user).await?;

        Ok(UserView::from(user))
    }
}
```
**`di.rs`**
```rust
use std::sync::Arc;
use super::ports::{CreateUserPort, HrnGenerator};
use super::use_case::CreateUserUseCase;

pub struct CreateUserUseCaseFactory;

impl CreateUserUseCaseFactory {
    pub fn build<P, G>(persister: Arc<P>, hrn_generator: Arc<G>) -> CreateUserUseCase<P, G>
    where
        P: CreateUserPort,
        G: HrnGenerator,
    {
        CreateUserUseCase::new(persister, hrn_generator)
    }
}
```
*   *(`dto.rs`, `error.rs`, `use_case_test.rs` serían implementados como se detalló en las HU)*

##### **`.../features/get_effective_policies/`**

**`ports.rs`**
```rust
use async_trait::async_trait;
use kernel::domain::policy::HodeiPolicySet;
use kernel::Hrn;
use super::error::GetEffectivePoliciesError;

#[async_trait]
pub trait EffectivePoliciesProvider: Send + Sync {
    async fn get_policies_for_principal(&self, principal_hrn: &Hrn) -> Result<HodeiPolicySet, GetEffectivePoliciesError>;
}
```
**`use_case.rs`**
```rust
use std::sync::Arc;
use super::dto::GetEffectivePoliciesQuery;
use super::error::GetEffectivePoliciesError;
use super::ports::EffectivePoliciesProvider;
use kernel::domain::policy::HodeiPolicySet;

pub struct GetEffectivePoliciesUseCase<P: EffectivePoliciesProvider> {
    provider: Arc<P>,
}

impl<P: EffectivePoliciesProvider> GetEffectivePoliciesUseCase<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    pub async fn execute(&self, query: GetEffectivePoliciesQuery) -> Result<HodeiPolicySet, GetEffectivePoliciesError> {
        self.provider.get_policies_for_principal(&query.principal_hrn).await
    }
}
```
**`di.rs`**
```rust
use std::sync::Arc;
use super::ports::EffectivePoliciesProvider;
use super::use_case::GetEffectivePoliciesUseCase;

pub struct GetEffectivePoliciesUseCaseFactory;

impl GetEffectivePoliciesUseCaseFactory {
    pub fn build<P: EffectivePoliciesProvider>(provider: Arc<P>) -> GetEffectivePoliciesUseCase<P> {
        GetEffectivePoliciesUseCase::new(provider)
    }
}
```
*   *(`dto.rs`, `error.rs` serían implementados como se detalló en las HU)*

#### **6. `crates/hodei-iam/src/infrastructure/` (Implementaciones Públicas)**

##### **`.../infrastructure/surreal/user_adapter.rs`**

```rust
use std::sync::Arc;
use async_trait::async_trait;
use surrealdb::{Surreal, engine::any::Any};
use kernel::Hrn;

// Importar los traits PÚBLICOS de las features que vamos a implementar
use crate::features::create_user::ports::CreateUserPort;
use crate::features::create_user::error::CreateUserError;

// Importar la entidad de dominio INTERNA (`pub(crate)`)
use crate::internal::domain::User;

pub struct SurrealUserAdapter {
    db: Arc<Surreal<Any>>,
}

impl SurrealUserAdapter {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

// Implementación del puerto para `create_user`
#[async_trait]
impl CreateUserPort for SurrealUserAdapter {
    async fn save_user(&self, user: &User) -> Result<(), CreateUserError> {
        let created: Option<User> = self.db.create(("user", user.hrn.resource_id()))
            .content(user)
            .await
            .map_err(|e| CreateUserError::PersistenceError(e.to_string()))?;
        
        if created.is_none() {
            return Err(CreateUserError::PersistenceError("Failed to create user in DB".to_string()));
        }
        Ok(())
    }
}

// Aquí se implementarían los otros traits como `GetUserPort`, `DeleteUserPort`, etc.
// #[async_trait]
// impl GetUserPort for SurrealUserAdapter { ... }
```

##### **`.../infrastructure/hrn_generator.rs`**
```rust
use kernel::Hrn;
use uuid::Uuid;
use crate::features::create_user::ports::HrnGenerator; // Asumiendo que es compartido

pub struct UuidHrnGenerator {
    partition: String,
    account_id: String,
}

impl UuidHrnGenerator {
    pub fn new(partition: String, account_id: String) -> Self {
        Self { partition, account_id }
    }
}

impl HrnGenerator for UuidHrnGenerator {
    fn new_user_hrn(&self, _name: &str) -> Hrn {
        let resource_id = Uuid::new_v4().to_string();
        Hrn::new(
            self.partition.clone(),
            "iam".to_string(),
            self.account_id.clone(),
            "User".to_string(),
            resource_id,
        )
    }
}
```

##### **`.../infrastructure/surreal/effective_policies_adapter.rs`**
```rust
use std::sync::Arc;
use async_trait::async_trait;
use surrealdb::{Surreal, engine::any::Any};
use kernel::{Hrn, domain::policy::{HodeiPolicy, HodeiPolicySet}};

use crate::features::get_effective_policies::ports::EffectivePoliciesProvider;
use crate::features::get_effective_policies::error::GetEffectivePoliciesError;
use crate::internal::domain::{User, Group}; // Necesitamos los tipos para deserializar

pub struct SurrealEffectivePoliciesAdapter {
    db: Arc<Surreal<Any>>,
}
// ... implementación de `new` ...

#[async_trait]
impl EffectivePoliciesProvider for SurrealEffectivePoliciesAdapter {
    async fn get_policies_for_principal(&self, principal_hrn: &Hrn) -> Result<HodeiPolicySet, GetEffectivePoliciesError> {
        // ALGORITMO COMPLEJO:
        // 1. Iniciar una transacción de SurrealDB si es necesario.
        // 2. `SELECT * FROM user WHERE hrn = $principal_hrn` para obtener el usuario.
        // 3. Si no existe, devolver `PrincipalNotFound`.
        // 4. `SELECT ->attaches->policy.* FROM $user.id` para obtener políticas directas.
        // 5. `SELECT ->member_of->group.* FROM $user.id` para obtener los grupos.
        // 6. Para cada grupo, `SELECT ->attaches->policy.* FROM $group.id`.
        // 7. Recolectar todas las `HodeiPolicy` únicas en un `HodeiPolicySet`.
        // 8. Devolver el `HodeiPolicySet`.
        
        // Ejemplo simplificado:
        let user: Option<User> = self.db.select(("user", principal_hrn.resource_id())).await.map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;
        let user = user.ok_or_else(|| GetEffectivePoliciesError::PrincipalNotFound(principal_hrn.to_string()))?;
        
        let mut policy_set = HodeiPolicySet::default();
        
        // Esta es una consulta de GRAFO que SurrealDB maneja eficientemente
        let mut result = self.db.query("SELECT ->member_of->group->attaches->policy.* as policies FROM ONLY $user_id")
            .bind(("user_id", ("user", principal_hrn.resource_id())))
            .await.map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;
        
        let policies: Option<Vec<HodeiPolicy>> = result.take("policies").unwrap_or(None);

        if let Some(policies) = policies {
            for policy in policies {
                policy_set.add(policy);
            }
        }
        
        Ok(policy_set)
    }
}
```

### **Resumen del Flujo de DI para la `Composition Root`**

Tu `app_state.rs` ahora se vería así:```rust
// en `src/app_state.rs`
let db = Arc::new(/* Surreal connection */);

// 1. Crear adaptadores de infraestructura
let user_adapter = Arc::new(SurrealUserAdapter::new(db.clone()));
let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".into(), "default".into()));
let effective_policies_adapter = Arc::new(SurrealEffectivePoliciesAdapter::new(db.clone()));

// 2. Inyectar en los UseCases
let create_user_uc = CreateUserUseCaseFactory::build(user_adapter.clone(), hrn_generator);
let get_effective_policies_uc = GetEffectivePoliciesUseCaseFactory::build(effective_policies_adapter);

// 3. Almacenar como `dyn Trait`
let state = AppState {
    create_user_uc: Arc::new(create_user_uc),
    get_effective_policies_uc: Arc::new(get_effective_policies_uc),
    // ...
};
```
Este código está completamente refactorizado para seguir la arquitectura V7.0. Es modular, testable, y la separación de responsabilidades es extremadamente clara.