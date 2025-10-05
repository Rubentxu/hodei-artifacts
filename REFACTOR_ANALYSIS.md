# 📊 Análisis de Refactorización: Estado Actual y Plan de Acción

**Fecha**: 2025-01-24  
**Objetivo**: Implementar arquitectura de monolito modular descomponible con aislamiento total de Cedar

---

## 🔍 1. ESTADO ACTUAL DEL CÓDIGO

### 1.1 Estructura de Workspace

```
hodei-artifacts/
├── crates/
│   ├── kernel/              ❌ Depende de cedar-policy (PROBLEMA)
│   ├── policies/            ✅ Único que debería depender de Cedar
│   ├── hodei-iam/           ❌ Depende de cedar-policy (PROBLEMA)
│   ├── hodei-organizations/ ❌ Depende de cedar-policy (PROBLEMA)
│   ├── hodei-authorizer/    ❌ Depende de cedar-policy (PROBLEMA)
│   └── [otros crates...]    ❌ Varios dependen de cedar-policy
└── src/ (hodei-artifacts-api) ❌ Depende de cedar-policy (PROBLEMA)
```

### 1.2 Dependencias de `cedar-policy` Detectadas

**Crates con dependencia directa a `cedar-policy` en Cargo.toml:**

1. ✅ `policies` - **CORRECTO** (debe ser el único)
2. ❌ `kernel` - **INCORRECTO** (shared kernel no debe depender de Cedar)
3. ❌ `hodei-iam` - **INCORRECTO**
4. ❌ `hodei-organizations` - **INCORRECTO**
5. ❌ `hodei-authorizer` - **INCORRECTO**
6. ❌ `artifact` - **INCORRECTO**
7. ❌ `repository` - **INCORRECTO**
8. ❌ `security` - **INCORRECTO**
9. ❌ `supply-chain` - **INCORRECTO**
10. ❌ `hodei-artifacts-api` (root) - **INCORRECTO**

**Total**: 10 crates dependen de `cedar-policy`, cuando solo 1 debería hacerlo.

### 1.3 Uso de Tipos Cedar en el Código

#### En `kernel/src/domain/entity.rs`:
```rust
use cedar_policy::{EntityTypeName, EntityUid, Policy, RestrictedExpression};

pub trait HodeiEntityType {
    fn cedar_entity_type_name() -> EntityTypeName { ... }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> { ... }
}

pub trait HodeiEntity {
    fn attributes(&self) -> HashMap<String, RestrictedExpression>;
    fn parents(&self) -> Vec<EntityUid> { ... }
    fn euid(&self) -> EntityUid { ... }
}
```

**Problema**: El kernel está completamente acoplado a Cedar.

#### En entidades de dominio (`hodei-iam`, `hodei-organizations`):
```rust
// hodei-iam/src/shared/domain/entities.rs
use cedar_policy::{EntityUid, RestrictedExpression};

impl HodeiEntity for User {
    fn attributes(&self) -> HashMap<String, RestrictedExpression> { ... }
    fn parents(&self) -> Vec<EntityUid> { ... }
}
```

**Problema**: Las entidades de dominio dependen directamente de tipos Cedar.

---

## 🚨 2. CÓDIGO LEGACY Y OBSOLETO DETECTADO

### 2.1 Código Obsoleto en `hodei-authorizer`

#### ❌ `src/authorizer.rs` - **ELIMINAR**
```rust
pub struct AuthorizerService<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> {
    iam_provider: IAM,
    org_provider: ORG,
    policy_evaluator: PolicyEvaluator,
}
```

**Razón**: Este fichero es el "viejo authorizer" que mezcla lógica. Debe ser reemplazado por el patrón de orquestación pura en `EvaluatePermissionsUseCase`.

#### ⚠️ `src/features/evaluate_permissions/ports.rs`
```rust
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) 
        -> EvaluatePermissionsResult<PolicySet>;
}
```

**Problema**: El trait devuelve `cedar_policy::PolicySet` directamente. Debe usar tipos agnósticos.

**Estado**: El trait existe pero su implementación vive en `hodei-organizations`. Necesita refactorización según el plan.

### 2.2 Implementaciones Legacy de Providers

#### ❌ `hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs`

Esta implementación es un **adapter obsoleto** que implementa directamente el trait del authorizer. Según la nueva arquitectura, debe:
1. Eliminarse como implementación directa del trait de authorizer
2. Reemplazarse por una implementación del puerto `ScpEvaluator` definido en `shared`

### 2.3 Código en `src/app_state.rs`

```rust
pub struct AppState {
    // ...
    pub authorization_engine: Arc<policies::shared::AuthorizationEngine>,
    // ...
    pub user_repo: Arc<dyn hodei_iam::ports::UserRepository>,
    pub group_repo: Arc<dyn hodei_iam::ports::GroupRepository>,
}
```

**Problemas**:
1. Expone repositorios directamente (rompe encapsulación)
2. Falta el `EvaluatePermissionsUseCase` del authorizer
3. No hay instancias de los evaluadores (`ScpEvaluator`, `IamPolicyEvaluator`)

---

## ✅ 3. ASPECTOS POSITIVOS DETECTADOS

### 3.1 Estructura VSA en Features
✅ Las features siguen correctamente la estructura VSA:
```
features/create_user/
├── mod.rs
├── use_case.rs
├── ports.rs
├── adapter.rs
├── error.rs
├── dto.rs
└── use_case_test.rs
```

### 3.2 Separación de Concerns
✅ Los bounded contexts están separados en crates independientes
✅ Existe un kernel compartido (aunque necesita refactorización)
✅ Ya existe `IamPolicyProvider` movido correctamente a `hodei-iam` (según docs)

### 3.3 Testing
✅ Tests unitarios presentes en features
✅ Uso de mocks con `mockall`

---

## 🎯 4. INCONSISTENCIAS CRÍTICAS DETECTADAS

### 4.1 Violación del Principio de Aislamiento

**Requisito**: Solo `policies` debe depender de `cedar-policy`

**Realidad**: 10 crates dependen de Cedar

**Impacto**: 
- Acoplamiento transversal masivo
- Imposible cambiar el motor de políticas sin tocar todo el código
- Violación de DDD (dominios dependen de infraestructura)

### 4.2 Inversión de Dependencias Incorrecta

**Actual**:
```
kernel → cedar-policy
   ↑
hodei-iam → cedar-policy
   ↑
hodei-authorizer → cedar-policy
```

**Deseado**:
```
kernel (agnóstico)
   ↑
hodei-iam (agnóstico) ← IamPolicyEvaluator trait
   ↑
hodei-authorizer (orquestador)
   ↓
policies → cedar-policy (ÚNICO con dependencia)
```

### 4.3 Falta de Value Objects en el Kernel

**Actual**: Se usan strings primitivos y tipos Cedar directamente

**Deseado**: Value Objects tipados:
- `ServiceName(String)` con validación
- `ResourceTypeName(String)` con validación
- `AttributeName(String)` con validación
- `AttributeValue` enum agnóstico

### 4.4 Traits en Lugar Incorrecto

**Problema**: `OrganizationBoundaryProvider` está definido en `hodei-authorizer`

**Correcto**: Debe ser `ScpEvaluator` definido en `shared/kernel`

---

## 📋 5. LISTA DE ARCHIVOS A ELIMINAR

### Archivos de Código Legacy

```
❌ crates/hodei-authorizer/src/authorizer.rs
❌ crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs
❌ crates/hodei-authorizer/src/ports.rs (si existe versión vieja)
```

### Dependencias a Eliminar de Cargo.toml

Remover `cedar-policy` de:
```
❌ crates/kernel/Cargo.toml
❌ crates/hodei-iam/Cargo.toml
❌ crates/hodei-organizations/Cargo.toml
❌ crates/hodei-authorizer/Cargo.toml
❌ crates/artifact/Cargo.toml
❌ crates/repository/Cargo.toml
❌ crates/security/Cargo.toml
❌ crates/supply-chain/Cargo.toml
❌ Cargo.toml (root binary)
```

---

## 🎯 6. PLAN DE IMPLEMENTACIÓN PRIORIZADO

### Fase 1: Crear Kernel Agnóstico (Épica 1) - CRÍTICO

**Prioridad**: 🔴 ALTA - Fundacional

#### HU-1.1: Definir Value Objects ✅ COMPLETADO
- [x] Crear `crates/kernel/src/domain/value_objects.rs`
- [x] Implementar `ServiceName`, `ResourceTypeName`, `AttributeName`
- [x] Tests de validación (35 tests passing)

#### HU-1.2: Definir `AttributeValue` Agnóstico ✅ COMPLETADO
- [x] Crear enum `AttributeValue` sin dependencias de Cedar
- [x] Implementar variantes: `Bool`, `Long`, `String`, `Set`, `Record`, `EntityRef`
- [x] Tests de serialización y conversiones (18 tests passing)

#### HU-1.3: Refactorizar Traits del Kernel ✅ COMPLETADO
- [x] Actualizar `HodeiEntityType` para usar Value Objects
- [x] Actualizar `HodeiEntity` para usar `AttributeValue`
- [x] Eliminar imports de `cedar-policy` en `kernel/src/domain/entity.rs`
- [x] Actualizar `AttributeType` enum para ser agnóstico
- [x] Actualizar `Hrn` para usar Value Objects y añadir métodos de acceso
- [x] Refactorizar `PolicyStorage` para usar strings en lugar de `cedar_policy::Policy`
- [x] Tests completos (75 tests passing)

#### HU-1.4: Actualizar Entidades de Dominio
- [ ] Refactorizar `User`, `Group` en `hodei-iam`
- [ ] Refactorizar `Account`, `OrganizationalUnit` en `hodei-organizations`
- [ ] Eliminar `cedar-policy` de dependencias de estos crates

#### HU-1.5: Definir Puertos de Evaluación en Kernel
- [ ] Crear `kernel/src/application/ports/authorization.rs`
- [ ] Definir `ScpEvaluator` trait
- [ ] Definir `IamPolicyEvaluator` trait
- [ ] Definir DTOs agnósticos (`EvaluationRequest`, `EvaluationDecision`)

#### HU-1.6: Sellar Bounded Contexts
- [ ] Hacer privado `mod shared;` en `hodei-iam/src/lib.rs`
- [ ] Hacer privado `mod shared;` en `hodei-organizations/src/lib.rs`

### Fase 2: Aislar `policies` como Traductor (Épica 2)

**Prioridad**: 🟠 MEDIA-ALTA - Aislamiento

#### HU-2.1: Implementar Traductor Agnóstico → Cedar
- [ ] Crear `crates/policies/src/translator.rs`
- [ ] Implementar `translate_attribute_value(AttributeValue) -> RestrictedExpression`
- [ ] Implementar `translate_to_cedar_entity(&dyn HodeiEntity) -> cedar_policy::Entity`

#### HU-2.2: Refactorizar `AuthorizationEngine`
- [ ] Actualizar interfaz pública para usar tipos agnósticos
- [ ] Usar traductor internamente
- [ ] Verificar que `policies` es el único con dependencia a Cedar

#### HU-2.3: Eliminar Features de Gestión de `policies`
- [ ] Eliminar `features/create_policy/` (mover a IAM u Organizations según corresponda)
- [ ] Eliminar `features/delete_policy/`
- [ ] Eliminar `features/update_policy/`
- [ ] Mantener solo `features/evaluate_policies/`

### Fase 3: Autonomizar Dominios (Épica 3)

**Prioridad**: 🟠 MEDIA - Autonomía

#### HU-3.1: `hodei-organizations` Gestiona SCPs
- [ ] Implementar `EvaluateScpsUseCase` que implementa `ScpEvaluator`
- [ ] Crear features de CRUD para SCPs
- [ ] Eliminar código obsoleto mencionado

#### HU-3.2: `hodei-iam` Gestiona Políticas de Identidad
- [ ] Implementar `EvaluateIamPoliciesUseCase` que implementa `IamPolicyEvaluator`
- [ ] Asegurar features de CRUD para políticas IAM

### Fase 4: Simplificar Authorizer (Épica 4)

**Prioridad**: 🟡 MEDIA - Orquestación

#### HU-4.1: Refactorizar `EvaluatePermissionsUseCase`
- [ ] Inyectar `Arc<dyn ScpEvaluator>` y `Arc<dyn IamPolicyEvaluator>`
- [ ] Implementar lógica de orquestación (SCP → Deny wins → IAM)
- [ ] Eliminar dependencias directas a otros bounded contexts
- [ ] Eliminar `authorizer.rs` obsoleto

### Fase 5: Componer Aplicación (Épica 5)

**Prioridad**: 🟢 BAJA - Integración

#### HU-5.1: Simplificar `AppState`
- [ ] Eliminar repositorios directos
- [ ] Exponer solo use cases necesarios para handlers

#### HU-5.2: Implementar Composition Root
- [ ] Refactorizar `build_app_state` en `src/lib.rs`
- [ ] Construir Schema global de Cedar
- [ ] Instanciar evaluadores con inyección de dependencias
- [ ] Cablear todo el grafo de dependencias

#### HU-5.3: Refactorizar Handlers de API
- [ ] Organizar por dominio (`api/iam.rs`, `api/organizations.rs`, `api/authorization.rs`)
- [ ] Simplificar a solo mapeo HTTP → DTO → UseCase
- [ ] Eliminar `policy_handlers.rs` obsoleto

---

## 📊 7. MÉTRICAS DE IMPACTO

### Antes (Estado Actual)
- 🔴 **Crates con dependencia a Cedar**: 10/11 (91%)
- 🔴 **Acoplamiento**: ALTO (tipos Cedar en 50+ archivos)
- 🔴 **Testabilidad**: MEDIA (mocks dependen de Cedar)
- 🟡 **Cumplimiento VSA**: 70% (estructura correcta, pero acoplamiento)

### Después (Estado Objetivo)
- 🟢 **Crates con dependencia a Cedar**: 1/11 (9%)
- 🟢 **Acoplamiento**: BAJO (solo `policies` conoce Cedar)
- 🟢 **Testabilidad**: ALTA (mocks agnósticos)
- 🟢 **Cumplimiento VSA**: 95% (estructura + desacoplamiento)

---

## 🚀 8. RECOMENDACIONES DE EJECUCIÓN

### Orden Recomendado

1. **Primero HU-1.1, 1.2, 1.3** (Value Objects + Traits agnósticos)
   - Sin esto, no se puede avanzar en nada más
   - Permite compilar con tipos agnósticos

2. **Luego HU-1.4** (Actualizar entidades)
   - Aplicar los nuevos traits a las entidades existentes
   - Eliminar dependencias de Cedar en dominios

3. **Después HU-2.1, 2.2** (Traductor en policies)
   - Aislar Cedar completamente
   - Verificar que solo `policies` depende de Cedar

4. **Continuar HU-1.5, 3.1, 3.2** (Evaluadores autónomos)
   - Implementar los puertos de evaluación
   - Hacer que cada dominio evalúe sus propias políticas

5. **Finalmente HU-4.1, 5.x** (Orquestador + API)
   - Cablear todo en el composition root
   - Exponer vía REST

### Estrategia de Testing

- ✅ Ejecutar `cargo check` después de cada HU
- ✅ Ejecutar `cargo clippy` para eliminar warnings
- ✅ Ejecutar `cargo test` o `cargo nextest run` después de cada fase
- ✅ Mantener cobertura de tests > 80%

### Checkpoints de Validación

**Checkpoint 1** (Después de Fase 1):
```bash
grep -r "use cedar_policy::" crates/kernel/ crates/hodei-iam/ crates/hodei-organizations/
# Resultado esperado: Sin matches
```

**Checkpoint 2** (Después de Fase 2):
```bash
grep "cedar-policy" crates/*/Cargo.toml | grep -v "crates/policies"
# Resultado esperado: Sin matches (excepto en policies)
```

**Checkpoint 3** (Después de Fase 3):
```bash
cargo build --all-features
cargo test --all-features
# Resultado esperado: Todo compila y tests pasan
```

---

## 📝 9. CONCLUSIONES

### Problemas Principales Identificados

1. **Acoplamiento Masivo a Cedar**: 91% de los crates dependen del motor de políticas
2. **Kernel Contaminado**: El shared kernel depende de detalles de implementación
3. **Código Legacy**: Existen implementaciones obsoletas que deben eliminarse
4. **Falta de Value Objects**: Uso de primitivos en lugar de tipos de dominio

### Beneficios Esperados

1. ✅ **Flexibilidad**: Poder cambiar Cedar por otro motor sin tocar dominios
2. ✅ **Testabilidad**: Mocks completamente agnósticos
3. ✅ **Desacoplamiento**: Bounded contexts verdaderamente independientes
4. ✅ **Preparación para Microservicios**: Cada crate puede extraerse fácilmente

### Riesgos y Mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Refactor rompe funcionalidad existente | ALTA | ALTO | Tests exhaustivos después de cada HU |
| Conversiones agnóstico→Cedar causan bugs | MEDIA | MEDIO | Tests de integración con casos reales |
| Tiempo de implementación mayor al estimado | ALTA | MEDIO | Implementar por fases, validar incrementalmente |

---

## ✅ 10. CHECKLIST DE VERIFICACIÓN FINAL

Usar este checklist al completar todas las épicas:

- [ ] `grep -r "use cedar_policy::" crates/` devuelve matches solo en `crates/policies/`
- [ ] `cargo check --workspace` compila sin errores
- [ ] `cargo clippy --workspace` no devuelve warnings
- [ ] `cargo test --workspace` todos los tests pasan
- [ ] `cargo nextest run --workspace` todos los tests pasan
- [ ] Cada bounded context (`hodei-iam`, `hodei-organizations`) tiene su `mod shared;` privado
- [ ] El `AppState` solo contiene use cases, no repositorios
- [ ] Los handlers de API solo hacen mapeo HTTP → UseCase
- [ ] Existe `ScpEvaluator` trait en `kernel/src/application/ports/`
- [ ] Existe `IamPolicyEvaluator` trait en `kernel/src/application/ports/`
- [ ] `policies/src/translator.rs` existe y contiene las conversiones
- [ ] No existe `hodei-authorizer/src/authorizer.rs`
- [ ] Cobertura de tests > 80%

---

**Estado del Documento**: ✅ COMPLETO  
**Última Actualización**: 2025-01-24 - HU-1.1 y HU-1.2 completadas

---

## 🎉 PROGRESO DE IMPLEMENTACIÓN

### ✅ Completado

#### Fase 1 - Kernel Agnóstico (3 de 6 HUs completadas)

**HU-1.1: Value Objects** ✅
- Creado `crates/kernel/src/domain/value_objects.rs`
- Implementados:
  - `ServiceName` con validación kebab-case
  - `ResourceTypeName` con validación PascalCase
  - `AttributeName` con validación de identificadores
  - `ValidationError` con tipos de error específicos
- 35 tests unitarios pasando
- Compilación sin errores ni warnings

**HU-1.2: AttributeValue Agnóstico** ✅
- Creado `crates/kernel/src/domain/attributes.rs`
- Implementado enum `AttributeValue` con 6 variantes:
  - `Bool`, `Long`, `String`, `Set`, `Record`, `EntityRef`
- Métodos de construcción, verificación y acceso
- Conversiones desde tipos Rust nativos (`From` traits)
- Serialización/deserialización JSON completa
- 18 tests unitarios pasando
- Soporte para estructuras anidadas

**Actualizado**: `kernel/src/domain/mod.rs`
- Expone `value_objects` y `attributes` públicamente
- Re-exports ergonómicos para consumidores

**HU-1.3: Refactorizar Traits del Kernel** ✅
- Refactorizado completamente `kernel/src/domain/entity.rs`:
  - `HodeiEntityType` ahora usa `ServiceName` y `ResourceTypeName` en lugar de `&'static str`
  - `HodeiEntity::attributes()` retorna `HashMap<AttributeName, AttributeValue>` agnóstico
  - `HodeiEntity::parent_hrns()` retorna `Vec<Hrn>` en lugar de `Vec<EntityUid>`
  - Eliminados métodos que retornaban tipos Cedar (`cedar_entity_type_name()`, `euid()`)
  - `ActionTrait` refactorizado para ser agnóstico
- Actualizado `AttributeType` enum:
  - Ahora es completamente agnóstico (Bool, Long, String, Set, Record, EntityRef)
  - Eliminada variante `Primitive` que era ambigua
  - Métodos constructores para cada tipo
- Refactorizado `kernel/src/domain/hrn.rs`:
  - Añadidos métodos de acceso públicos: `service()`, `resource_id()`, `resource_type()`, etc.
  - Eliminada dependencia de `cedar-policy`
  - `to_euid()` reemplazado por `entity_type_name()` y `entity_uid_string()` (agnósticos)
  - Método `for_entity_type<T>()` actualizado para usar Value Objects
- Refactorizado `PolicyStorage` trait:
  - Ahora trabaja con strings de políticas en lugar de `cedar_policy::Policy`
  - Métodos actualizados: `save_policy(id, text)`, `get_policy_by_id()`, etc.
- 22 tests nuevos para entity.rs y hrn.rs
- Documentación completa con ejemplos

**Verificaciones**:
```bash
✅ cargo check -p kernel     # Sin errores
✅ cargo test -p kernel      # 75 tests pasando (22 más que antes)
✅ cargo clippy -p kernel    # Solo 2 warnings menores de estilo
✅ grep "cedar_policy" kernel/src/domain/*.rs  # Sin matches!
```

### 🔄 En Progreso

**HU-1.4: Actualizar Entidades de Dominio** - PRÓXIMO
- Refactorizar `User`, `Group` en `hodei-iam` para implementar los nuevos traits
- Refactorizar `Account`, `OrganizationalUnit`, `ServiceControlPolicy` en `hodei-organizations`
- Eliminar `cedar-policy` de dependencias de estos crates
- Actualizar implementaciones de `HodeiEntityType` y `HodeiEntity`

### ⏳ Pendiente

- HU-1.5: Definir Puertos de Evaluación en Kernel
- HU-1.6: Sellar Bounded Contexts
- Fase 2: Aislar `policies` como Traductor (Épica 2)
- Fase 3: Autonomizar Dominios (Épica 3)
- Fase 4: Simplificar Authorizer (Épica 4)
- Fase 5: Componer Aplicación (Épica 5)

**Próximo Paso**: Continuar con **HU-1.4: Actualizar Entidades de Dominio**

### 📊 Estadísticas de Progreso

**Fase 1 - Kernel Agnóstico**: 50% completado (3/6 HUs)
- ✅ HU-1.1: Value Objects
- ✅ HU-1.2: AttributeValue Agnóstico
- ✅ HU-1.3: Refactorizar Traits del Kernel
- ⏳ HU-1.4: Actualizar Entidades de Dominio
- ⏳ HU-1.5: Definir Puertos de Evaluación
- ⏳ HU-1.6: Sellar Bounded Contexts

**Tests en kernel**: 75 (↑22 desde último checkpoint)
**Dependencias de Cedar en kernel**: 0 ✅ (objetivo logrado)