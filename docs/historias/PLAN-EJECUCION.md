# Plan de Ejecución - Historias de Usuario Pendientes

## 🎯 Objetivo General

Completar las historias de usuario pendientes identificadas en la auditoría arquitectónica, priorizando por impacto y dependencias.

## 📊 Estado Actual (Resumen)

### ✅ Completado (60%)
- Historia 1: Shared Kernel implementado (`crates/kernel/`)
- Historia 2: Encapsulamiento de Bounded Contexts (95% completo)
- Historia 3: Separación CRUD de Políticas (5 features independientes)

### 🟡 Pendiente (40%)
- **Historia 6**: Eliminar Warnings del Compilador (0% - CRÍTICA)
- **Historia 4**: Eliminar Acoplamiento en Infraestructura (0% - ALTA)
- **Historia 5**: Errores Específicos (60% - 3 features pendientes)

## 🚀 Orden de Ejecución

```
┌─────────────────────────────────────────────────────────────┐
│  SPRINT ACTUAL (2-3 días)                                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Día 1 - Mañana (2-4h)                                     │
│  ┌───────────────────────────────────────────────────┐     │
│  │ ⚡ HISTORIA 6: Eliminar Warnings                   │     │
│  │   - 14+ warnings identificados                    │     │
│  │   - Limpieza de código muerto                     │     │
│  │   - Resolver imports no usados                    │     │
│  │   META: cargo clippy --all sin warnings           │     │
│  └───────────────────────────────────────────────────┘     │
│                                                             │
│  Día 1-2 (8-16h)                                           │
│  ┌───────────────────────────────────────────────────┐     │
│  │ 🟡 HISTORIA 4: Refactorizar OrganizationBoundary  │     │
│  │   - Eliminar acoplamiento infra → aplicación     │     │
│  │   - Implementar lógica directa con repos         │     │
│  │   - Tests unitarios + integración                │     │
│  │   META: Clean Architecture restaurada            │     │
│  └───────────────────────────────────────────────────┘     │
│                                                             │
│  Día 3 (6-8h)                                              │
│  ┌───────────────────────────────────────────────────┐     │
│  │ 🟡 HISTORIA 5: Errores Específicos                │     │
│  │   - add_user_to_group: AddUserToGroupError       │     │
│  │   - create_group: CreateGroupError               │     │
│  │   - create_user: CreateUserError                 │     │
│  │   META: anyhow::Error eliminado de 3 features    │     │
│  └───────────────────────────────────────────────────┘     │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 📋 Historia 6: Eliminar Warnings del Compilador ⚡

**Prioridad:** CRÍTICA  
**Tiempo Estimado:** 2-4 horas  
**Bloqueante:** No  

### Paso a Paso

```bash
# 1. Generar lista completa de warnings
cargo clippy --all 2>&1 | tee warnings.txt

# 2. Categorizar warnings
#    - unused imports (5)
#    - unused variables (1)
#    - dead code (8)
#    - redundant closures (4+)
```

### Checklist de Tareas

- [ ] **Grupo 1: Imports No Usados (10 min)**
  ```bash
  # Archivos a editar:
  - crates/hodei-iam/src/features/create_policy_new/validator.rs:12
  - crates/hodei-iam/src/features/get_policy/use_case.rs:3
  ```
  - [ ] Eliminar `ValidationWarning`
  - [ ] Eliminar `async_trait::async_trait`

- [ ] **Grupo 2: Variables No Usadas (5 min)**
  ```bash
  # Archivo:
  - crates/hodei-iam/src/features/list_policies/dto.rs:85
  ```
  - [ ] Cambiar `let limit =` → `let _limit =` o usar la variable

- [ ] **Grupo 3: Código Muerto - Domain Actions (30 min)**
  ```bash
  # Archivo:
  - crates/hodei-iam/src/internal/domain/actions.rs
  ```
  - [ ] Evaluar si las actions se usarán en el futuro
  - [ ] Si SÍ: Agregar `#[allow(dead_code)]` con comentario explicativo
  - [ ] Si NO: Eliminar las structs no usadas

- [ ] **Grupo 4: Código Muerto - Errors (15 min)**
  ```bash
  # Archivo:
  - crates/hodei-iam/src/internal/application/ports/errors.rs:82
  ```
  - [ ] Evaluar `PolicyRepositoryError`
  - [ ] Si no se usa: Eliminar o marcar con `#[allow(dead_code)]`

- [ ] **Grupo 5: Mocks No Usados (30 min)**
  ```bash
  # Archivos:
  - crates/hodei-iam/src/features/create_policy_new/mocks.rs
  ```
  - [ ] `MockPolicyValidator`: Marcar con `#[cfg(test)]` y `#[allow(dead_code)]`
  - [ ] `MockCreatePolicyPort`: Marcar con `#[cfg(test)]` y `#[allow(dead_code)]`
  - [ ] Métodos asociados no usados: Marcar con `#[allow(dead_code)]`

- [ ] **Grupo 6: Closures Redundantes (20 min)**
  ```bash
  # Buscar y simplificar:
  .map(|x| f(x))  →  .map(f)
  ```
  - [ ] Identificar con: `cargo clippy --all 2>&1 | grep "redundant closure"`
  - [ ] Aplicar fix automático: `cargo clippy --fix --allow-dirty`

- [ ] **Grupo 7: Policies Crate (15 min)**
  ```bash
  # Archivo: crates/policies/
  ```
  - [ ] Resolver warning de `into_iter()`
  - [ ] Aplicar: `cargo clippy --fix --lib -p policies`

### Comandos de Verificación

```bash
# Durante el desarrollo
cargo check --all

# Al finalizar cada grupo
cargo clippy --all

# Verificación final (DEBE PASAR SIN WARNINGS)
cargo clippy --all -- -D warnings

# Tests (DEBEN SEGUIR PASANDO)
cargo nextest run
```

### Criterios de Aceptación

```bash
✅ cargo check --all        # Sin errores
✅ cargo clippy --all       # 0 warnings
✅ cargo nextest run        # 100% tests pasan
✅ git diff                 # Revisar cambios son conservadores
```

---

## 📋 Historia 4: Eliminar Acoplamiento Infraestructura 🟡

**Prioridad:** ALTA  
**Tiempo Estimado:** 8-16 horas  
**Bloqueante:** No (pero importante arquitecturalmente)  

### Contexto del Problema

```rust
// PROBLEMA ACTUAL en organization_boundary_provider.rs
// ❌ Infraestructura invoca caso de uso (inversión de dependencias)

impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, hrn: &Hrn) -> Result<PolicySet, Error> {
        // ❌ Crea el caso de uso desde infraestructura
        let use_case = get_effective_scps_use_case(repos...);
        
        // ❌ Ejecuta lógica de aplicación desde infraestructura
        let result = use_case.execute(command).await?;
    }
}
```

### Solución Propuesta

```rust
// ✅ SOLUCIÓN: Infraestructura implementa lógica directamente

impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, hrn: &Hrn) -> Result<PolicySet, Error> {
        // ✅ Usa repositorios inyectados
        // 1. Determinar tipo (Account o OU)
        // 2. Cargar entidad
        // 3. Recorrer jerarquía
        // 4. Recolectar SCPs
        // 5. Construir PolicySet
    }
}
```

### Paso a Paso Detallado

#### Fase 1: Análisis y Documentación (2h)

- [ ] **Tarea 1.1**: Leer y documentar algoritmo actual
  ```bash
  # Archivo a analizar:
  crates/hodei-organizations/src/features/get_effective_scps/use_case.rs
  ```
  - [ ] Extraer pseudocódigo del algoritmo
  - [ ] Identificar dependencias (repos usados)
  - [ ] Documentar casos edge

- [ ] **Tarea 1.2**: Diseñar estructura del adaptador refactorizado
  ```rust
  // Diseño propuesto:
  pub struct SurrealOrganizationBoundaryProvider {
      scp_repo: Arc<SurrealScpRepository>,
      account_repo: Arc<SurrealAccountRepository>,
      ou_repo: Arc<SurrealOuRepository>,
  }
  ```

#### Fase 2: Implementación (4-6h)

- [ ] **Tarea 2.1**: Refactorizar constructor
  ```rust
  impl SurrealOrganizationBoundaryProvider {
      pub fn new(
          scp_repo: Arc<SurrealScpRepository>,
          account_repo: Arc<SurrealAccountRepository>,
          ou_repo: Arc<SurrealOuRepository>,
      ) -> Self {
          Self { scp_repo, account_repo, ou_repo }
      }
  }
  ```

- [ ] **Tarea 2.2**: Implementar `determine_entity_type`
  ```rust
  fn determine_entity_type(hrn: &Hrn) -> Result<EntityType, Error> {
      // Parsear HRN y determinar si es Account o OU
  }
  ```

- [ ] **Tarea 2.3**: Implementar `load_entity`
  ```rust
  async fn load_entity(&self, hrn: &Hrn, entity_type: EntityType) 
      -> Result<Entity, Error> {
      // Usar account_repo o ou_repo según tipo
  }
  ```

- [ ] **Tarea 2.4**: Implementar `traverse_hierarchy`
  ```rust
  async fn traverse_hierarchy(&self, start_hrn: &Hrn) 
      -> Result<Vec<Hrn>, Error> {
      // Recorrer desde entidad hasta root
      // Recolectar HRNs de OUs en el camino
  }
  ```

- [ ] **Tarea 2.5**: Implementar `collect_scps`
  ```rust
  async fn collect_scps(&self, entity_hrns: Vec<Hrn>) 
      -> Result<Vec<Hrn>, Error> {
      // Para cada entidad, obtener SCPs adjuntos
  }
  ```

- [ ] **Tarea 2.6**: Implementar `build_policy_set`
  ```rust
  async fn build_policy_set(&self, scp_hrns: Vec<Hrn>) 
      -> Result<PolicySet, Error> {
      // Cargar contenido de SCPs
      // Parsear y construir PolicySet de Cedar
  }
  ```

- [ ] **Tarea 2.7**: Orquestar en `get_effective_scps_for`
  ```rust
  async fn get_effective_scps_for(&self, resource_hrn: &Hrn) 
      -> Result<PolicySet, Error> {
      let entity_type = self.determine_entity_type(resource_hrn)?;
      let entity = self.load_entity(resource_hrn, entity_type).await?;
      let hierarchy = self.traverse_hierarchy(&entity.parent_ou_hrn).await?;
      let scp_hrns = self.collect_scps(hierarchy).await?;
      let policy_set = self.build_policy_set(scp_hrns).await?;
      Ok(policy_set)
  }
  ```

#### Fase 3: Testing (4-6h)

- [ ] **Tarea 3.1**: Crear mocks para repositorios
  ```bash
  # Archivo nuevo:
  crates/hodei-organizations/src/internal/infrastructure/surreal/mocks.rs
  ```
  - [ ] `MockScpRepository`
  - [ ] `MockAccountRepository`
  - [ ] `MockOuRepository`

- [ ] **Tarea 3.2**: Tests unitarios - Caso simple
  ```rust
  #[tokio::test]
  async fn test_account_with_direct_scps() {
      // Account sin OU padre, con SCPs directos
  }
  ```

- [ ] **Tarea 3.3**: Tests unitarios - Jerarquía simple
  ```rust
  #[tokio::test]
  async fn test_account_in_ou_with_scps() {
      // Account → OU → Root
      // SCPs en Account y OU
  }
  ```

- [ ] **Tarea 3.4**: Tests unitarios - Jerarquía profunda
  ```rust
  #[tokio::test]
  async fn test_deep_hierarchy() {
      // Account → OU3 → OU2 → OU1 → Root
      // SCPs en múltiples niveles
  }
  ```

- [ ] **Tarea 3.5**: Tests unitarios - Edge cases
  ```rust
  #[tokio::test]
  async fn test_account_without_parent_ou() { }
  
  #[tokio::test]
  async fn test_ou_without_scps() { }
  
  #[tokio::test]
  async fn test_entity_not_found() { }
  ```

- [ ] **Tarea 3.6**: Tests de integración con testcontainers
  ```bash
  # Archivo:
  crates/hodei-organizations/tests/integration_organization_boundary_test.rs
  ```
  - [ ] Setup: Levantar SurrealDB
  - [ ] Crear jerarquía completa
  - [ ] Adjuntar SCPs en cada nivel
  - [ ] Verificar políticas efectivas

#### Fase 4: Limpieza y Verificación (2h)

- [ ] **Tarea 4.1**: Eliminar imports de caso de uso
  ```rust
  // Eliminar estas líneas:
  // use crate::features::get_effective_scps::di::get_effective_scps_use_case;
  // use crate::features::get_effective_scps::dto::GetEffectiveScpsCommand;
  ```

- [ ] **Tarea 4.2**: Verificar que el caso de uso sigue funcionando
  ```bash
  cargo nextest run -p hodei-organizations get_effective_scps
  ```

- [ ] **Tarea 4.3**: Verificar compilación y warnings
  ```bash
  cargo check -p hodei-organizations
  cargo clippy -p hodei-organizations
  ```

- [ ] **Tarea 4.4**: Ejecutar todos los tests
  ```bash
  cargo nextest run
  ```

### Criterios de Aceptación

```
✅ SurrealOrganizationBoundaryProvider NO importa GetEffectiveScpsUseCase
✅ Repositorios inyectados en constructor
✅ Lógica implementada directamente en el adaptador
✅ Tests unitarios con mocks > 90% coverage
✅ Tests de integración pasan
✅ Tests de regresión del caso de uso pasan
✅ No hay warnings
✅ Todos los tests pasan
```

---

## 📋 Historia 5: Errores Específicos 🟡

**Prioridad:** MEDIA  
**Tiempo Estimado:** 6-8 horas  
**Bloqueante:** No  

### Features Afectadas

1. `add_user_to_group` → `AddUserToGroupError`
2. `create_group` → `CreateGroupError`
3. `create_user` → `CreateUserError`

### Template de Implementación (Repetir 3 veces)

```rust
// 1. Crear error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XxxError {
    #[error("Entity not found: {0}")]
    NotFound(String),
    
    #[error("Invalid HRN: {0}")]
    InvalidHrn(String),
    
    #[error("Transaction failed: {0}")]
    TransactionError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

// 2. Actualizar use_case.rs
pub async fn execute(&self, cmd: Command) -> Result<Output, XxxError> {
    // Mapear errores
    let hrn = Hrn::from_string(&cmd.hrn)
        .ok_or_else(|| XxxError::InvalidHrn(cmd.hrn.clone()))?;
}

// 3. Actualizar tests
#[tokio::test]
async fn test_error_invalid_hrn() {
    let result = use_case.execute(cmd).await;
    assert!(matches!(result, Err(XxxError::InvalidHrn(_))));
}

// 4. Actualizar mod.rs
pub use error::XxxError;
```

### Checklist Detallado

#### Feature 1: add_user_to_group (2h)

- [ ] Crear `error.rs` con `AddUserToGroupError`
- [ ] Variantes: UserNotFound, GroupNotFound, InvalidUserHrn, InvalidGroupHrn, TransactionError, RepositoryError
- [ ] Actualizar `use_case.rs` línea 27
- [ ] Mapear errores en `execute_in_transaction`
- [ ] Actualizar `use_case_test.rs` (6+ tests de error)
- [ ] Actualizar `mod.rs` para re-exportar error
- [ ] Verificar: `cargo test -p hodei-iam add_user_to_group`

#### Feature 2: create_group (2h)

- [ ] Crear `error.rs` con `CreateGroupError`
- [ ] Variantes: DuplicateGroup, InvalidGroupName, InvalidHrn, TransactionError, RepositoryError
- [ ] Actualizar `use_case.rs` línea 27
- [ ] Mapear errores en `execute_in_transaction`
- [ ] Actualizar `use_case_test.rs` (6+ tests de error)
- [ ] Actualizar `mod.rs` para re-exportar error
- [ ] Verificar: `cargo test -p hodei-iam create_group`

#### Feature 3: create_user (2h)

- [ ] Crear `error.rs` con `CreateUserError`
- [ ] Variantes: DuplicateUser, InvalidUserName, InvalidEmail, InvalidHrn, TransactionError, RepositoryError
- [ ] Actualizar `use_case.rs` línea 27
- [ ] Mapear errores en `execute_in_transaction`
- [ ] Actualizar `use_case_test.rs` (7+ tests de error)
- [ ] Actualizar `mod.rs` para re-exportar error
- [ ] Verificar: `cargo test -p hodei-iam create_user`

#### Integración Final (2h)

- [ ] Actualizar `lib.rs` para re-exportar los 3 errores
- [ ] Verificar traits `Send + Sync` en los 3 errores
- [ ] Actualizar handlers HTTP si existen
- [ ] Ejecutar: `cargo clippy -p hodei-iam`
- [ ] Ejecutar: `cargo nextest run -p hodei-iam`
- [ ] Buscar `anyhow::Error` restantes: `rg "anyhow::Error" crates/hodei-iam/`

### Criterios de Aceptación

```
✅ 3 archivos error.rs creados
✅ 3 casos de uso actualizados
✅ 19+ tests de error agregados (6+6+7)
✅ No más anyhow::Error en firmas públicas
✅ Errores re-exportados en lib.rs
✅ Todos los tests pasan
✅ No hay warnings
```

---

## 🎯 Comandos Rápidos de Referencia

```bash
# Estado del proyecto
cargo check --all
cargo clippy --all
cargo nextest run

# Verificación de calidad (DEBE PASAR AL FINAL)
cargo clippy --all -- -D warnings

# Tests por crate
cargo nextest run -p kernel
cargo nextest run -p hodei-iam
cargo nextest run -p hodei-organizations
cargo nextest run -p hodei-authorizer

# Buscar problemas específicos
rg "anyhow::Error" crates/
rg "TODO" crates/
rg "FIXME" crates/

# Limpieza
cargo clean
```

---

## 📈 Tracking de Progreso

### Historia 6: Eliminar Warnings
- [ ] Grupo 1: Imports no usados (10min)
- [ ] Grupo 2: Variables no usadas (5min)
- [ ] Grupo 3: Código muerto - Actions (30min)
- [ ] Grupo 4: Código muerto - Errors (15min)
- [ ] Grupo 5: Mocks no usados (30min)
- [ ] Grupo 6: Closures redundantes (20min)
- [ ] Grupo 7: Policies crate (15min)
- [ ] Verificación final

**Progreso**: 0/8 (0%)

### Historia 4: Acoplamiento Infraestructura
- [ ] Fase 1: Análisis (2h)
- [ ] Fase 2: Implementación (4-6h)
- [ ] Fase 3: Testing (4-6h)
- [ ] Fase 4: Limpieza (2h)

**Progreso**: 0/4 (0%)

### Historia 5: Errores Específicos
- [ ] Feature 1: add_user_to_group (2h)
- [ ] Feature 2: create_group (2h)
- [ ] Feature 3: create_user (2h)
- [ ] Integración final (2h)

**Progreso**: 0/4 (0%)

---

## ✅ Checklist Final del Sprint

Al completar todas las historias:

```bash
# 1. Calidad de código
□ cargo check --all           # Sin errores
□ cargo clippy --all          # Sin warnings
□ rg "anyhow::Error" crates/  # Solo en internal, no en public API
□ rg "TODO|FIXME" crates/     # Documentar pendientes

# 2. Tests
□ cargo nextest run                      # 100% pasan
□ cargo nextest run --lib -p hodei-iam   # Tests unitarios
□ cargo nextest run --test '*' -p hodei-iam  # Tests integración

# 3. Arquitectura
□ No hay acoplamiento infra → aplicación
□ Bounded contexts encapsulados
□ Errores específicos en API pública

# 4. Documentación
□ historias-usuario.md actualizado
□ README.md actualizado si es necesario
□ Comentarios en código donde sea necesario
```

---

**Fecha Inicio**: [FECHA]  
**Fecha Fin Estimada**: [FECHA + 3 días]  
**Responsable**: [NOMBRE]  
**Estado Actual**: 🟡 PENDIENTE