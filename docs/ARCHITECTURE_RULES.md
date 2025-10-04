# Reglas de Oro de Arquitectura - Hodei Artifacts

**Estado:** 🔒 REGLAS OBLIGATORIAS  
**Última actualización:** 2024

---

## 🎯 Principio Fundamental

> **"Cada crate expone SOLO casos de uso (features) con Commands/Queries/DTOs.  
> Las entidades de dominio, repositorios y servicios son INTERNOS y NUNCA se exponen."**

---

## ✅ REGLAS OBLIGATORIAS

### Regla 1: Solo Exportar Casos de Uso

**✅ HACER:**
```rust
// crate/src/lib.rs
pub use features::{
    create_entity::CreateEntityUseCase,
    get_something::{
        GetSomethingUseCase,
        GetSomethingQuery,
        GetSomethingResponse,
    },
};
```

**❌ NO HACER:**
```rust
// crate/src/lib.rs
pub use shared::domain::{User, Group, Account};  // ❌ NUNCA exportar entidades
pub use shared::infrastructure::repositories::UserRepository;  // ❌ NUNCA exportar repos
```

---

### Regla 2: Entidades de Dominio son Internas

**✅ HACER:**
```rust
// crate/src/shared/domain/mod.rs
pub(crate) mod user;      // ← INTERNO al crate
pub(crate) mod group;     // ← INTERNO al crate

pub(crate) use user::User;       // ← Solo visible dentro del crate
pub(crate) use group::Group;     // ← Solo visible dentro del crate
```

**❌ NO HACER:**
```rust
// crate/src/shared/domain/mod.rs
pub mod user;       // ❌ Hace que User sea público
pub use user::User; // ❌ Exporta la entidad
```

---

### Regla 3: Casos de Uso Devuelven DTOs, NO Entidades

**✅ HACER:**
```rust
// feature/dto.rs
pub struct GetUserResponse {
    pub user_id: String,
    pub username: String,
    pub email: String,
}

// feature/use_case.rs
impl GetUserUseCase {
    pub async fn execute(&self, query: GetUserQuery) 
        -> Result<GetUserResponse, Error>  // ✅ DTO
    {
        let user = self.repo.find_by_id(&query.user_id).await?;  // Entidad interna
        
        // Convertir entidad a DTO
        Ok(GetUserResponse {
            user_id: user.id.to_string(),
            username: user.username,
            email: user.email,
        })
    }
}
```

**❌ NO HACER:**
```rust
impl GetUserUseCase {
    pub async fn execute(&self, query: GetUserQuery) 
        -> Result<User, Error>  // ❌ Devuelve entidad interna
    {
        self.repo.find_by_id(&query.user_id).await
    }
}
```

---

### Regla 4: Comunicación Entre Crates SOLO vía Casos de Uso

**✅ HACER:**
```rust
// En hodei-authorizer
use hodei_iam::GetEffectivePoliciesForPrincipalUseCase;  // ✅ Caso de uso
use hodei_iam::GetEffectivePoliciesQuery;                 // ✅ Query DTO
use hodei_iam::EffectivePoliciesResponse;                 // ✅ Response DTO

pub struct EvaluatePermissionsUseCase {
    iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,  // ✅ Inyectar caso de uso
}

impl EvaluatePermissionsUseCase {
    pub async fn execute(&self, request: Request) -> Result<Response, Error> {
        let query = GetEffectivePoliciesQuery { ... };
        let response = self.iam_use_case.execute(query).await?;  // ✅ Usar caso de uso
        // ...
    }
}
```

**❌ NO HACER:**
```rust
// En hodei-authorizer
use hodei_iam::domain::User;           // ❌ NO importar entidades
use hodei_iam::repositories::UserRepo; // ❌ NO importar repositorios

pub struct EvaluatePermissionsUseCase {
    user_repo: Arc<dyn UserRepo>,  // ❌ NO inyectar repositorios de otros crates
}
```

---

### Regla 5: NO Crear Providers/Wrappers Innecesarios

**✅ HACER:**
```rust
// Usar casos de uso directamente
pub struct OrchestratorUseCase {
    other_crate_use_case: Arc<SomeUseCase>,  // ✅ Directo
}
```

**❌ NO HACER:**
```rust
// NO crear un provider que wrappea un caso de uso
pub trait MyProvider {
    async fn do_something(&self) -> Result<...>;
}

pub struct MyProviderImpl {
    use_case: Arc<SomeUseCase>,  // ❌ Wrapper innecesario
}

impl MyProvider for MyProviderImpl {
    async fn do_something(&self) -> Result<...> {
        self.use_case.execute(...).await  // ❌ Solo delega
    }
}
```

---

### Regla 6: PolicyStorage y Detalles de Implementación son Internos

**✅ HACER (en crate policies):**
```rust
// policies/src/shared/mod.rs
pub use application::{AuthorizationEngine, EngineBuilder};
pub use domain::{HodeiEntity, Principal, Resource, Action};

// ❌ NO exportar:
// pub use domain::ports::PolicyStorage;  
// pub use infrastructure::SurrealMemStorage;
```

**Otros crates NO deben:**
```rust
// ❌ NO HACER en otros crates
use policies::domain::ports::PolicyStorage;      // ❌ Detalle interno
use policies::infrastructure::SurrealMemStorage; // ❌ Detalle interno
```

---

### Regla 7: Construcción de Dependencias en Application Layer

**✅ HACER (en main.rs o app layer):**
```rust
// src/main.rs
async fn main() -> Result<()> {
    // ✅ La aplicación construye todo
    let iam_use_case = build_iam_use_case().await?;
    let org_use_case = build_org_use_case().await?;
    let auth_engine = build_authorization_engine().await?;
    
    // ✅ Inyectar en el orquestador
    let orchestrator = EvaluatePermissionsUseCase::new(
        Arc::new(iam_use_case),
        Arc::new(org_use_case),
        Arc::new(auth_engine),
    );
    
    Ok(())
}
```

**❌ NO HACER (en un crate intermedio):**
```rust
// ❌ NO construir dependencias de otros crates
impl SomeUseCase {
    pub fn new() -> Self {
        let storage = SurrealMemStorage::new(...).await?;  // ❌ Conoce detalles internos
        let engine = AuthorizationEngine { ... };          // ❌ Construye manualmente
        // ...
    }
}
```

---

### Regla 8: Estructura de Feature (VSA)

**✅ HACER:**
```
crate/src/features/some_feature/
├── mod.rs           # Exports públicos
├── use_case.rs      # Lógica de negocio
├── dto.rs           # Commands, Queries, Responses
├── error.rs         # Errores específicos
├── ports.rs         # Traits (interfaces)
├── adapter.rs       # Implementaciones
├── di.rs            # Dependency Injection
├── use_case_test.rs # Tests unitarios
└── mocks.rs         # Mocks para tests
```

**❌ NO HACER:**
```
crate/src/features/some_feature/
├── service.rs       # ❌ NO usar nombres genéricos
├── controller.rs    # ❌ NO romper VSA
├── handler.rs       # ❌ NO mezclar capas
└── repository.rs    # ❌ Los repos van en infrastructure
```

---

## 🔍 CHECKLIST DE VALIDACIÓN

Antes de hacer commit, verifica:

### ✅ Exports Públicos
```bash
# Verificar que lib.rs solo exporta casos de uso
grep -n "pub use" crate/src/lib.rs

# ✅ Debe tener: pub use features::...
# ❌ NO debe tener: pub use domain::...
# ❌ NO debe tener: pub use infrastructure::...
```

### ✅ Entidades Internas
```bash
# Verificar que entidades son pub(crate)
grep -r "pub use.*::User" crate/src/

# ✅ Resultado esperado: 0 matches (o solo pub(crate))
```

### ✅ Sin Importaciones Incorrectas
```bash
# Verificar que hodei-authorizer NO importa entidades
grep -r "use hodei_iam::.*domain::" hodei-authorizer/src/
grep -r "use hodei_organizations::.*domain::" hodei-authorizer/src/

# ✅ Resultado esperado: 0 matches
```

### ✅ Sin PolicyStorage Expuesto
```bash
# Verificar que PolicyStorage no se usa fuera de policies
grep -r "PolicyStorage" --include="*.rs" --exclude-dir=policies

# ✅ Resultado esperado: 0 matches en código de producción
```

### ✅ Compilación Limpia
```bash
# Verificar compilación sin errores
cargo check --workspace

# Verificar sin warnings
cargo clippy --workspace --all-targets

# ✅ Debe pasar sin errores ni warnings
```

---

## 🚨 SEÑALES DE ALERTA

### 🔴 Arquitectura Incorrecta Detectada Si:

1. **Un crate exporta entidades de dominio**
   ```rust
   pub use domain::User;  // 🔴 ALERTA
   ```

2. **Un caso de uso devuelve entidades**
   ```rust
   async fn execute(...) -> Result<User, Error>  // 🔴 ALERTA
   ```

3. **Un crate importa entidades de otro crate**
   ```rust
   use other_crate::domain::Entity;  // 🔴 ALERTA
   ```

4. **Se exponen detalles de implementación**
   ```rust
   pub use infrastructure::SurrealMemStorage;  // 🔴 ALERTA
   ```

5. **Se crean providers innecesarios**
   ```rust
   pub trait WrapperProvider { ... }  // 🔴 ALERTA (si solo delega a caso de uso)
   ```

---

## 📖 PATRONES CORRECTOS

### Patrón 1: Crear Nueva Feature

```rust
// 1. Crear estructura de directorios
crate/src/features/new_feature/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── ports.rs
└── adapter.rs

// 2. Definir DTOs
// dto.rs
pub struct NewFeatureCommand { ... }
pub struct NewFeatureResponse { ... }

// 3. Implementar caso de uso
// use_case.rs
pub struct NewFeatureUseCase { ... }
impl NewFeatureUseCase {
    pub async fn execute(&self, cmd: NewFeatureCommand) 
        -> Result<NewFeatureResponse, Error> 
    { ... }
}

// 4. Exportar desde mod.rs
// mod.rs
pub use dto::{NewFeatureCommand, NewFeatureResponse};
pub use use_case::NewFeatureUseCase;
pub use error::NewFeatureError;

// 5. Exportar desde lib.rs
// lib.rs
pub use features::new_feature::{
    NewFeatureUseCase,
    NewFeatureCommand,
    NewFeatureResponse,
};
```

### Patrón 2: Usar Feature de Otro Crate

```rust
// En Cargo.toml
[dependencies]
other_crate = { path = "../other_crate" }

// En código
use other_crate::SomeFeatureUseCase;    // ✅ Caso de uso
use other_crate::SomeFeatureQuery;      // ✅ Query
use other_crate::SomeFeatureResponse;   // ✅ Response

pub struct MyUseCase {
    other_use_case: Arc<SomeFeatureUseCase>,  // ✅ Inyectar
}

impl MyUseCase {
    pub async fn execute(&self, ...) -> Result<...> {
        let query = SomeFeatureQuery { ... };
        let response = self.other_use_case.execute(query).await?;  // ✅ Usar
        // ...
    }
}
```

---

## 📚 REFERENCIAS

- `docs/architecture-final-correct.md` - Arquitectura completa
- `docs/encapsulation-boundaries.md` - Guía de encapsulación
- `docs/single-responsibility-refactoring.md` - Plan de refactorización
- `docs/refactoring-complete-summary.md` - Resumen de cambios

---

## ⚖️ RESPONSABILIDADES

### Desarrollador
- ✅ Seguir estas reglas en todo momento
- ✅ Revisar checklist antes de commit
- ✅ No exportar entidades de dominio
- ✅ Comunicarse solo via casos de uso

### Revisor de Código
- ✅ Verificar cumplimiento de reglas
- ✅ Rechazar PRs que violen encapsulación
- ✅ Validar que solo se exportan casos de uso
- ✅ Confirmar que tests pasan

### Arquitecto
- ✅ Mantener estas reglas actualizadas
- ✅ Resolver dudas de arquitectura
- ✅ Validar nuevos patrones
- ✅ Documentar excepciones (si las hay)

---

## 🎓 RESUMEN EN 3 PUNTOS

1. **Exporta SOLO casos de uso** (features) con DTOs
2. **Marca entidades como `pub(crate)`** (internas)
3. **Comunícate entre crates via casos de uso** (NO entidades)

---

**Estas reglas son OBLIGATORIAS y NO opcionales.**  
**Cualquier violación debe ser corregida antes de merge.**

---

**Versión:** 1.0  
**Estado:** 🔒 OBLIGATORIO  
**Última revisión:** 2024