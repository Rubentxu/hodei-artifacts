# Plan de Implementación: hodei-iam

## Resumen Ejecutivo

Este documento define el plan de implementación completo para el bounded context `hodei-iam`, siguiendo estrictamente las reglas de arquitectura definidas en `AGENTS.md` y delegando las capacidades de validación y evaluación de políticas a `hodei-policies`.

## Estado Actual

### ✅ Features Implementadas y Correctas

| ID | Feature | Archivos VSA | Estado | Notas |
|----|---------|--------------|--------|-------|
| R-001 | `create_policy` | Completos | ✅ OK | Delega validación a `hodei-policies::ValidatePolicyPort` |
| R-002 | `get_policy` | Completos | ✅ OK | Lectura simple de política |
| R-003 | `list_policies` | Completos | ✅ OK | Listado de políticas |
| R-004 | `update_policy` | Completos | ✅ OK | Actualización de política |
| R-005 | `delete_policy` | Completos | ✅ OK | Eliminación de política |
| R-006 | `create_user` | Completos | ✅ OK | Creación de usuario |
| R-007 | `create_group` | Completos | ✅ OK | Creación de grupo |
| R-008 | `add_user_to_group` | Completos | ✅ OK | Gestión de membresía |
| R-009 | `get_effective_policies` | Completos | ✅ OK | Obtiene políticas efectivas del principal |

### ⚠️ Features que Requieren Refactorización

| Feature | Problema | Solución Requerida |
|---------|----------|-------------------|
| `evaluate_iam_policies` | Implementación stub, no delega a hodei-policies | Refactorizar para usar `EvaluatePoliciesUseCase` |

### ❌ Features Faltantes (CRUD Incompleto)

#### Gestión de Users
- `get_user` - Leer usuario individual
- `update_user` - Actualizar usuario
- `delete_user` - Eliminar usuario
- `list_users` - Listar usuarios (opcional)

#### Gestión de Groups
- `get_group` - Leer grupo individual
- `update_group` - Actualizar grupo
- `delete_group` - Eliminar grupo
- `list_groups` - Listar grupos (opcional)

#### Gestión de Relaciones
- `remove_user_from_group` - Remover usuario de grupo

## Principios Arquitectónicos Obligatorios

### 1. Delegación a hodei-policies

**hodei-iam NO debe:**
- ❌ Importar Cedar directamente
- ❌ Implementar lógica de evaluación de políticas
- ❌ Construir schemas de Cedar
- ❌ Traducir entidades a formato Cedar

**hodei-iam SÍ debe:**
- ✅ Delegar validación de políticas a `hodei_policies::features::validate_policy::ValidatePolicyPort`
- ✅ Delegar evaluación a `hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase`
- ✅ Proporcionar entidades que implementen `kernel::HodeiEntity`
- ✅ Gestionar el almacenamiento de políticas IAM (CRUD)
- ✅ Obtener políticas efectivas para un principal

### 2. Estructura VSA Obligatoria

Cada feature en `src/features/{nombre}/` debe contener:

```
{nombre}/
├── mod.rs              # Exporta el módulo
├── use_case.rs         # Lógica de negocio orquestal
├── ports.rs            # Traits específicos (ISP)
├── dto.rs              # Comandos, Queries, Views
├── error.rs            # Errores específicos
├── di.rs               # Factory para DI
├── mocks.rs            # (Opcional) Mocks para tests
└── use_case_test.rs    # Tests unitarios con mocks
```

### 3. Segregación de Interfaces (ISP)

- Cada `ports.rs` debe definir traits **mínimos y específicos**
- Un trait debe tener la mínima cantidad de métodos posibles
- Evitar traits "god object" (ej: `UserRepository` con 10 métodos)
- Preferir: `CreateUserPort`, `GetUserPort`, `UpdateUserPort`, etc.

### 4. Cero Acoplamiento entre Bounded Contexts

```rust
// ❌ PROHIBIDO
use hodei_organizations::SomeType;

// ✅ CORRECTO - A través del kernel
use kernel::HodeiEntity;

// ✅ CORRECTO - A través de puertos públicos
use hodei_policies::features::validate_policy::ValidatePolicyPort;
```

## Plan de Implementación

### 🔴 Fase 1: Refactorización Crítica (ALTA PRIORIDAD)

#### HU-IAM-R-010: Refactorizar `evaluate_iam_policies`

**Objetivo:** Delegar correctamente la evaluación de políticas a `hodei-policies`.

**Tareas:**
1. ✅ Verificar estado actual del código
2. Modificar `use_case.rs`:
   - Importar `hodei_policies::features::evaluate_policies::{EvaluatePoliciesUseCase, dto::*}`
   - Instanciar `EvaluatePoliciesUseCase`
   - Construir `EvaluatePoliciesCommand` con:
     - `request`: `AuthorizationRequest` con principal, action, resource
     - `policies`: `HodeiPolicySet` de políticas efectivas
     - `entities`: Vec de entidades (principal, resource)
   - Mapear resultado a `EvaluationDecision`
3. Crear port `PrincipalResolverPort`:
   - Método: `resolve_principal(&Hrn) -> Result<Box<dyn HodeiEntity>>`
   - Para obtener la entidad User/Group desde el HRN
4. Crear port `ResourceResolverPort`:
   - Método: `resolve_resource(&Hrn) -> Result<Box<dyn HodeiEntity>>`
   - Para obtener la entidad Resource desde el HRN (puede ser mock inicial)
5. Actualizar `di.rs` para inyectar dependencias
6. Actualizar `use_case_test.rs`:
   - Mockear `PolicyFinderPort`
   - Mockear `PrincipalResolverPort`
   - Mockear `ResourceResolverPort`
   - Verificar que se construye correctamente el comando
   - Verificar decisiones Allow/Deny

**Criterios de Aceptación:**
- ✅ El código compila sin errores
- ✅ No hay warnings de clippy
- ✅ Todos los tests pasan
- ✅ No hay imports directos a Cedar
- ✅ La evaluación se delega completamente a hodei-policies
- ✅ Los tests verifican la delegación correcta

**Archivos Afectados:**
- `features/evaluate_iam_policies/use_case.rs`
- `features/evaluate_iam_policies/ports.rs` (nuevos traits)
- `features/evaluate_iam_policies/dto.rs` (si necesario)
- `features/evaluate_iam_policies/di.rs`
- `features/evaluate_iam_policies/mocks.rs`
- `features/evaluate_iam_policies/use_case_test.rs`

---

### 🟡 Fase 2: CRUD Completo de Users (MEDIA PRIORIDAD)

#### HU-IAM-R-011: Implementar `get_user`

**Objetivo:** Leer un usuario individual por HRN.

**Estructura VSA:**
```
features/get_user/
├── mod.rs
├── use_case.rs
├── ports.rs          # GetUserPort trait
├── dto.rs            # GetUserQuery, UserView
├── error.rs          # GetUserError
├── di.rs             # GetUserUseCaseFactory
├── mocks.rs
└── use_case_test.rs
```

**Tareas:**
1. Crear estructura de directorios
2. Implementar `GetUserPort` trait en `ports.rs`:
   ```rust
   #[async_trait]
   pub trait GetUserPort: Send + Sync {
       async fn get_by_hrn(&self, hrn: &Hrn) -> Result<User, GetUserError>;
   }
   ```
3. Implementar `GetUserUseCase` en `use_case.rs`
4. Implementar DTOs en `dto.rs`
5. Implementar errores en `error.rs`
6. Implementar factory en `di.rs`
7. Crear mocks en `mocks.rs`
8. Implementar tests en `use_case_test.rs`
9. Implementar adaptador en `infrastructure/surreal/user_adapter.rs`

**Criterios de Aceptación:**
- ✅ Estructura VSA completa
- ✅ Código compila sin errores ni warnings
- ✅ Tests unitarios pasan (100% coverage del use case)
- ✅ Port ISP segregado (solo 1 método)
- ✅ Usa tracing en lugar de println!

---

#### HU-IAM-R-012: Implementar `update_user`

**Objetivo:** Actualizar un usuario existente.

**Estructura VSA:**
```
features/update_user/
├── mod.rs
├── use_case.rs
├── ports.rs          # UpdateUserPort trait
├── dto.rs            # UpdateUserCommand, UserView
├── error.rs          # UpdateUserError
├── di.rs             # UpdateUserUseCaseFactory
├── mocks.rs
└── use_case_test.rs
```

**Tareas:**
1. Crear estructura de directorios
2. Implementar `UpdateUserPort` trait en `ports.rs`:
   ```rust
   #[async_trait]
   pub trait UpdateUserPort: Send + Sync {
       async fn update(&self, hrn: &Hrn, updates: UserUpdates) -> Result<User, UpdateUserError>;
   }
   ```
3. Implementar `GetUserPort` para verificación (puede reutilizar de get_user)
4. Implementar `UpdateUserUseCase` en `use_case.rs`
5. Implementar DTOs en `dto.rs`
6. Implementar errores en `error.rs`
7. Implementar factory en `di.rs`
8. Crear mocks en `mocks.rs`
9. Implementar tests en `use_case_test.rs`
10. Implementar adaptador en `infrastructure/surreal/user_adapter.rs`

**Criterios de Aceptación:**
- ✅ Estructura VSA completa
- ✅ Código compila sin errores ni warnings
- ✅ Tests unitarios pasan
- ✅ Verifica que el usuario existe antes de actualizar
- ✅ Ports ISP segregados

---

#### HU-IAM-R-013: Implementar `delete_user`

**Objetivo:** Eliminar un usuario existente.

**Estructura VSA:**
```
features/delete_user/
├── mod.rs
├── use_case.rs
├── ports.rs          # DeleteUserPort trait
├── dto.rs            # DeleteUserCommand
├── error.rs          # DeleteUserError
├── di.rs             # DeleteUserUseCaseFactory
├── mocks.rs
└── use_case_test.rs
```

**Tareas:**
1. Crear estructura de directorios
2. Implementar `DeleteUserPort` trait en `ports.rs`:
   ```rust
   #[async_trait]
   pub trait DeleteUserPort: Send + Sync {
       async fn delete(&self, hrn: &Hrn) -> Result<(), DeleteUserError>;
   }
   ```
3. Implementar `GetUserPort` para verificación
4. Implementar `DeleteUserUseCase` en `use_case.rs`
5. Implementar DTOs en `dto.rs`
6. Implementar errores en `error.rs`
7. Implementar factory en `di.rs`
8. Crear mocks en `mocks.rs`
9. Implementar tests en `use_case_test.rs`
10. Implementar adaptador en `infrastructure/surreal/user_adapter.rs`

**Criterios de Aceptación:**
- ✅ Estructura VSA completa
- ✅ Código compila sin errores ni warnings
- ✅ Tests unitarios pasan
- ✅ Verifica que el usuario existe antes de eliminar
- ✅ Ports ISP segregados

---

#### HU-IAM-R-014: Implementar `list_users` (OPCIONAL)

**Objetivo:** Listar usuarios con paginación y filtros.

**Estructura VSA:**
```
features/list_users/
├── mod.rs
├── use_case.rs
├── ports.rs          # ListUsersPort trait
├── dto.rs            # ListUsersQuery, UsersListView
├── error.rs          # ListUsersError
├── di.rs             # ListUsersUseCaseFactory
├── mocks.rs
└── use_case_test.rs
```

**Tareas:**
1. Crear estructura de directorios
2. Implementar `ListUsersPort` trait con paginación
3. Implementar `ListUsersUseCase`
4. Implementar DTOs con filtros y paginación
5. Implementar errores
6. Implementar factory
7. Crear mocks
8. Implementar tests
9. Implementar adaptador

**Criterios de Aceptación:**
- ✅ Estructura VSA completa
- ✅ Soporta paginación
- ✅ Soporta filtros básicos
- ✅ Tests unitarios pasan

---

### 🟡 Fase 3: CRUD Completo de Groups (MEDIA PRIORIDAD)

#### HU-IAM-R-015: Implementar `get_group`
#### HU-IAM-R-016: Implementar `update_group`
#### HU-IAM-R-017: Implementar `delete_group`
#### HU-IAM-R-018: Implementar `list_groups` (OPCIONAL)

**Nota:** Seguir el mismo patrón que las features de Users (R-011 a R-014).

---

### 🟢 Fase 4: Gestión de Relaciones (BAJA PRIORIDAD)

#### HU-IAM-R-019: Implementar `remove_user_from_group`

**Objetivo:** Remover un usuario de un grupo (inverso de `add_user_to_group`).

**Estructura VSA:**
```
features/remove_user_from_group/
├── mod.rs
├── use_case.rs
├── ports.rs          # RemoveUserFromGroupPort trait
├── dto.rs            # RemoveUserFromGroupCommand
├── error.rs          # RemoveUserFromGroupError
├── di.rs             # RemoveUserFromGroupUseCaseFactory
├── mocks.rs
└── use_case_test.rs
```

**Tareas:**
1. Crear estructura de directorios
2. Implementar ports (GetUserPort, GetGroupPort, RemoveUserFromGroupPort)
3. Implementar use case
4. Implementar DTOs y errores
5. Implementar factory y mocks
6. Implementar tests
7. Implementar adaptador

**Criterios de Aceptación:**
- ✅ Estructura VSA completa
- ✅ Verifica que el usuario y el grupo existen
- ✅ Verifica que el usuario es miembro del grupo
- ✅ Tests unitarios pasan

---

## Arquitectura de Delegación a hodei-policies

### Flujo de Validación de Políticas

```
┌─────────────────────────────────────────────────────┐
│  hodei-iam::features::create_policy::CreatePolicyUC │
└────────────────────┬────────────────────────────────┘
                     │
                     │ 1. ValidatePolicyCommand
                     ▼
┌─────────────────────────────────────────────────────┐
│  hodei-policies::features::validate_policy::        │
│  ValidatePolicyPort                                  │
│  (implementado por ValidatePolicyUseCase)           │
└────────────────────┬────────────────────────────────┘
                     │
                     │ 2. Delega a Cedar
                     ▼
           ┌──────────────────┐
           │  Cedar Validator  │
           └──────────────────┘
```

### Flujo de Evaluación de Políticas

```
┌─────────────────────────────────────────────────────────┐
│  hodei-iam::features::evaluate_iam_policies::           │
│  EvaluateIamPoliciesUseCase                             │
└────────────────────┬────────────────────────────────────┘
                     │
                     │ 1. Obtiene políticas efectivas
                     ▼
┌─────────────────────────────────────────────────────────┐
│  PolicyFinderPort::get_effective_policies()             │
│  (implementado por SurrealPolicyAdapter)                │
└────────────────────┬────────────────────────────────────┘
                     │
                     │ 2. Obtiene entidades
                     ▼
┌─────────────────────────────────────────────────────────┐
│  PrincipalResolverPort::resolve_principal()             │
│  ResourceResolverPort::resolve_resource()               │
│  (implementados por SurrealUserAdapter, etc.)           │
└────────────────────┬────────────────────────────────────┘
                     │
                     │ 3. EvaluatePoliciesCommand
                     ▼
┌─────────────────────────────────────────────────────────┐
│  hodei-policies::features::evaluate_policies::          │
│  EvaluatePoliciesUseCase                                │
└────────────────────┬────────────────────────────────────┘
                     │
                     │ 4. Delega a Cedar
                     ▼
           ┌──────────────────┐
           │  Cedar Authorizer │
           └──────────────────┘
```

## Checklist de Verificación por Feature

Antes de considerar una feature como completa, verificar:

- [ ] El código compila sin errores (`cargo check`)
- [ ] No hay warnings (`cargo clippy -- -D warnings`)
- [ ] Todos los tests pasan (`cargo nextest run`)
- [ ] La feature tiene todos los archivos VSA requeridos
- [ ] Los ports están segregados (ISP - mínimo de métodos)
- [ ] Las dependencias se inyectan via traits
- [ ] No hay acoplamiento directo con otros bounded contexts
- [ ] Los tests unitarios están implementados con mocks
- [ ] Se usa tracing para logging en lugar de println!
- [ ] La API pública está exportada en `api.rs`
- [ ] El módulo `internal/` es `pub(crate)` y sellado
- [ ] Se usan abstracciones del kernel (`Hrn`, `HodeiEntity`, etc.)

## Orden de Implementación Sugerido

1. **HU-IAM-R-010** (evaluate_iam_policies) - CRÍTICO
2. **HU-IAM-R-011** (get_user) - Base para otros CRUDs
3. **HU-IAM-R-012** (update_user)
4. **HU-IAM-R-013** (delete_user)
5. **HU-IAM-R-015** (get_group)
6. **HU-IAM-R-016** (update_group)
7. **HU-IAM-R-017** (delete_group)
8. **HU-IAM-R-019** (remove_user_from_group)
9. **HU-IAM-R-014** (list_users) - OPCIONAL
10. **HU-IAM-R-018** (list_groups) - OPCIONAL

## Dependencias entre Features

```
evaluate_iam_policies (R-010)
  ├── Depende de: get_effective_policies (R-009) ✅
  └── Depende de: hodei-policies ✅

get_user (R-011)
  └── Sin dependencias

update_user (R-012)
  └── Depende de: get_user (R-011)

delete_user (R-013)
  └── Depende de: get_user (R-011)

remove_user_from_group (R-019)
  ├── Depende de: get_user (R-011)
  └── Depende de: get_group (R-015)
```

## Notas Importantes

1. **No crear `ports/` a nivel de crate:** Los ports deben estar dentro de cada feature (`features/{nombre}/ports.rs`) siguiendo ISP.

2. **Exceptions permitidas:** Si múltiples features comparten el mismo port (ej: varias features necesitan `GetUserPort`), se puede:
   - Opción A: Duplicar el trait en cada feature (preferido para ISP puro)
   - Opción B: Crear `ports/` a nivel de crate SOLO para ese trait específico compartido (excepción justificada)

3. **Infrastructure compartida:** Los adaptadores en `infrastructure/` SÍ pueden implementar múltiples ports de diferentes features.

4. **Testing:** Cada feature debe tener cobertura del 100% en su `use_case_test.rs` usando mocks.

## Métricas de Éxito

Al completar este plan:
- ✅ 100% de features compilan sin errores ni warnings
- ✅ 100% de tests pasan
- ✅ CRUD completo para Users y Groups
- ✅ Delegación completa a hodei-policies para validación/evaluación
- ✅ Cero acoplamiento entre bounded contexts
- ✅ Arquitectura VSA estricta en todas las features
- ✅ ISP respetado en todos los ports

## Conclusión

Este plan garantiza que `hodei-iam` cumpla con:
- Arquitectura VSA estricta
- Delegación correcta a `hodei-policies`
- ISP en todos los ports
- Testing completo
- Cero acoplamiento entre bounded contexts
- CRUD completo para todas las entidades

Cada fase puede ser implementada de forma incremental, permitiendo validar la arquitectura en cada paso.