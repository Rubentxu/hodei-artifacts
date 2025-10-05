# Plan de Acción Definitivo: Implementación del Monolito Modular Agnóstico al Motor de Políticas

**Fecha:** 2025-01-XX  
**Estado:** 🔴 Correcciones Críticas Requeridas  
**Objetivo:** Implementar arquitectura de monolito modular con aislamiento total de `cedar-policy`

---

## 🎯 Visión Arquitectónica

Construir un **monolito modular descomponible** donde:

1. Cada `crate` es un **bounded context autónomo**
2. Dueño de sus propios datos y lógicas de negocio
3. Comunicación vía **orquestación y delegación síncrona** a través de interfaces abstractas
4. **`cedar-policy` es un detalle de implementación** completamente encapsulado en el crate `policies`
5. Invisible para el resto del sistema
6. Preparado para futura extracción a microservicios o cambio de motor de autorización

---

## 📊 Análisis del Estado Actual

### ✅ Lo Implementado Correctamente

#### 1. Kernel de Dominio Agnóstico (Épica 1: 85%)

**HU-1.1 a HU-1.5: ✅ COMPLETADAS**

```
crates/kernel/src/domain/
├── value_objects.rs  ✅ ServiceName, ResourceTypeName, AttributeName
├── attributes.rs     ✅ AttributeValue enum agnóstico
├── entity.rs         ✅ HodeiEntityType, HodeiEntity traits
├── hrn.rs            ✅ Identificador único global
└── mod.rs

crates/kernel/src/application/ports/
└── authorization.rs  ✅ ScpEvaluator, IamPolicyEvaluator traits
```

**Logros:**
- ✅ Value Objects con validación tipada
- ✅ 72 tests unitarios en value objects y attributes
- ✅ Traits 100% agnósticos (sin dependencias Cedar)
- ✅ Puertos de evaluación bien definidos

**Pendiente:**
- ❌ HU-1.6: Sellar módulos `shared` (hacer privados)

---

#### 2. Hodei-Authorizer como Orquestador Puro (Épica 4: 100%)

**HU-4.1: ✅ COMPLETADA**

```rust
// crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
    iam_evaluator: Arc<dyn IamPolicyEvaluator>,     // ✅ Trait abstracto
    org_evaluator: Arc<dyn ScpEvaluator>,            // ✅ Trait abstracto
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
}
```

**Logros:**
- ✅ Depende solo de traits, NO de implementaciones concretas
- ✅ Lógica de orquestación AWS correcta (SCP → IAM)
- ✅ Cross-cutting concerns integrados
- ✅ 9 tests unitarios
- ✅ **Arquitectura perfecta** - No requiere cambios

---

#### 3. Dominios Gestionan sus Propias Políticas

**✅ hodei-iam:**
```
crates/hodei-iam/src/features/
├── create_policy/          ✅ Gestión de políticas IAM
│   ├── use_case.rs
│   ├── ports.rs
│   ├── adapter.rs
│   └── dto.rs
├── create_user/            ✅ Gestión de usuarios
├── create_group/           ✅ Gestión de grupos
└── evaluate_iam_policies/  ⚠️ Existe pero incompleto
```

**✅ hodei-organizations:**
```
crates/hodei-organizations/src/features/
├── create_scp/            ✅ Gestión de SCPs
├── create_account/        ✅ Gestión de cuentas
├── create_ou/             ✅ Gestión de OUs
└── evaluate_scps/         ❌ NO EXISTE (crítico)
```

---

### 🔴 Problemas Críticos Detectados

#### ❌ PROBLEMA #1: `policies` tiene features de gestión (VIOLACIÓN ARQUITECTÓNICA)

**Estado Actual:**
```bash
crates/policies/src/features/
├── create_policy/              ❌ NO DEBE EXISTIR
├── batch_eval/                 ❌ Legacy, eliminar
├── evaluate_policies/          ❌ Legacy, eliminar
├── policy_analysis/            ❌ Legacy, eliminar
├── policy_playground/          ❌ Legacy, eliminar
├── policy_playground_traces/   ❌ Legacy, eliminar
└── validate_policy/            ⚠️ Evaluar si es útil compartido
```

**Problema:**
- HU-2.3 dice **explícitamente**: "Eliminar TODOS los directorios de features"
- `policies` debe ser **SOLO biblioteca de evaluación**
- Gestión de políticas es responsabilidad de cada dominio

**Estado Correcto:**
```
crates/policies/src/
├── lib.rs
└── shared/
    ├── application/
    │   └── engine.rs           # AuthorizationEngine (evaluación Cedar)
    └── infrastructure/
        ├── translator.rs       # ❌ NO EXISTE (crítico)
        └── validator.rs        # Opcional: validación sintáctica
```

**Acción Requerida:**
```bash
# ELIMINAR features incorrectas
rm -rf crates/policies/src/features/create_policy/
rm -rf crates/policies/src/features/batch_eval/
rm -rf crates/policies/src/features/evaluate_policies/
rm -rf crates/policies/src/features/policy_analysis/
rm -rf crates/policies/src/features/policy_playground/
rm -rf crates/policies/src/features/policy_playground_traces/

# EVALUAR validate_policy (puede ser útil como utilidad)
# Si se mantiene, moverlo a shared/infrastructure/
```

---

#### ❌ PROBLEMA #2: No existe el Traductor Cedar (BLOQUEANTE CRÍTICO)

**Ubicación esperada:** `crates/policies/src/shared/infrastructure/translator.rs`  
**Estado:** ❌ **NO EXISTE**

**Impacto:**
- Bloquea HU-2.1 (Épica 2)
- Impide que dominios evalúen políticas con tipos agnósticos
- Sin traductor, no hay forma de usar Cedar sin acoplamiento

**Funciones Requeridas:**

```rust
// crates/policies/src/shared/infrastructure/translator.rs
use kernel::{AttributeValue, HodeiEntity, AttributeName};
use cedar_policy::{RestrictedExpression, Entity, EntityUid};

/// Error de traducción
#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    #[error("Invalid attribute value: {0}")]
    InvalidAttributeValue(String),
    #[error("Invalid entity: {0}")]
    InvalidEntity(String),
    #[error("Unsupported type: {0}")]
    UnsupportedType(String),
}

/// Traduce AttributeValue agnóstico a RestrictedExpression de Cedar
pub fn translate_attribute_value(
    value: &AttributeValue
) -> Result<RestrictedExpression, TranslatorError> {
    match value {
        AttributeValue::Bool(b) => {
            Ok(RestrictedExpression::new_bool(*b))
        }
        AttributeValue::Long(n) => {
            Ok(RestrictedExpression::new_long(*n))
        }
        AttributeValue::String(s) => {
            Ok(RestrictedExpression::new_string(s.clone()))
        }
        AttributeValue::Set(values) => {
            let cedar_values: Result<Vec<_>, _> = values
                .iter()
                .map(translate_attribute_value)
                .collect();
            Ok(RestrictedExpression::new_set(cedar_values?))
        }
        AttributeValue::Record(map) => {
            let mut cedar_map = std::collections::HashMap::new();
            for (k, v) in map {
                cedar_map.insert(k.clone(), translate_attribute_value(v)?);
            }
            Ok(RestrictedExpression::new_record(cedar_map))
        }
        AttributeValue::EntityRef(hrn_str) => {
            // Parsear HRN y crear EntityUid
            let uid = EntityUid::from_str(hrn_str)
                .map_err(|e| TranslatorError::InvalidEntity(e.to_string()))?;
            Ok(RestrictedExpression::new_entity_uid(uid))
        }
    }
}

/// Traduce HodeiEntity agnóstico a Entity de Cedar
pub fn translate_to_cedar_entity(
    entity: &dyn HodeiEntity
) -> Result<Entity, TranslatorError> {
    let hrn = entity.hrn();
    let attributes = entity.attributes();
    
    // Convertir HRN a EntityUid de Cedar
    let uid = EntityUid::from_str(&hrn.to_string())
        .map_err(|e| TranslatorError::InvalidEntity(e.to_string()))?;
    
    // Traducir atributos
    let mut cedar_attrs = std::collections::HashMap::new();
    for (name, value) in attributes {
        let cedar_value = translate_attribute_value(&value)?;
        cedar_attrs.insert(name.to_string(), cedar_value);
    }
    
    // Traducir parents
    let parents: Result<Vec<EntityUid>, _> = entity
        .parent_hrns()
        .iter()
        .map(|hrn| EntityUid::from_str(&hrn.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TranslatorError::InvalidEntity(e.to_string()))?;
    
    // Construir entidad Cedar
    Ok(Entity::new(uid, cedar_attrs, parents))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn translate_bool() {
        let value = AttributeValue::bool(true);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }
    
    #[test]
    fn translate_long() {
        let value = AttributeValue::long(42);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }
    
    #[test]
    fn translate_string() {
        let value = AttributeValue::string("test");
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }
    
    #[test]
    fn translate_set() {
        let value = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::long(2),
        ]);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }
    
    #[test]
    fn translate_nested_record() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("key".to_string(), AttributeValue::string("value"));
        
        let mut outer = std::collections::HashMap::new();
        outer.insert("nested".to_string(), AttributeValue::record(inner));
        
        let value = AttributeValue::record(outer);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }
    
    // TODO: Más tests para entity translation, error cases, etc.
}
```

---

#### ❌ PROBLEMA #3: AuthorizationEngine NO tiene API agnóstica

**Estado Actual:** Engine acepta tipos Cedar directamente  
**Estado Requerido:** Engine debe aceptar traits `HodeiEntity`

**Refactor Requerido:**

```rust
// crates/policies/src/shared/application/engine.rs

use kernel::{HodeiEntity, AttributeValue};
use crate::shared::infrastructure::translator;

/// Request agnóstico para evaluación
pub struct EngineRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: &'a str,
    pub resource: &'a dyn HodeiEntity,
    pub context: std::collections::HashMap<String, AttributeValue>,
}

pub struct AuthorizationEngine {
    authorizer: cedar_policy::Authorizer,
    policies: cedar_policy::PolicySet,
    entities: cedar_policy::Entities,
}

impl AuthorizationEngine {
    /// API PÚBLICA AGNÓSTICA - No expone tipos Cedar
    pub fn is_authorized(
        &self,
        request: EngineRequest
    ) -> Result<bool, EngineError> {
        // 1. Traducir entidades agnósticas a Cedar
        let principal_cedar = translator::translate_to_cedar_entity(request.principal)?;
        let resource_cedar = translator::translate_to_cedar_entity(request.resource)?;
        let action_uid = cedar_policy::EntityUid::from_str(request.action)?;
        
        // 2. Construir request Cedar (INTERNO)
        let cedar_request = cedar_policy::Request::new(
            principal_cedar.uid().clone(),
            action_uid,
            resource_cedar.uid().clone(),
            cedar_policy::Context::empty(), // TODO: traducir context
        );
        
        // 3. Evaluar con Cedar (INTERNO)
        let response = self.authorizer.is_authorized(&cedar_request, &self.policies, &self.entities);
        
        // 4. Retornar decisión simple (sin exponer tipos Cedar)
        Ok(response.decision() == cedar_policy::Decision::Allow)
    }
    
    /// Cargar políticas desde strings Cedar DSL
    pub fn load_policies(
        &mut self,
        policy_texts: Vec<String>
    ) -> Result<(), EngineError> {
        // Parsear y cargar políticas Cedar
        // Esto es INTERNO, no se expone
        todo!()
    }
    
    /// Registrar entidades en el store
    pub fn register_entities(
        &mut self,
        entities: Vec<&dyn HodeiEntity>
    ) -> Result<(), EngineError> {
        // Traducir y almacenar entidades
        for entity in entities {
            let cedar_entity = translator::translate_to_cedar_entity(entity)?;
            self.entities.insert(cedar_entity);
        }
        Ok(())
    }
}
```

---

#### ❌ PROBLEMA #4: Cedar acoplado en Dominios

**hodei-organizations (9 archivos):**
```
❌ src/shared/domain/account.rs:6              use cedar_policy
❌ src/shared/domain/ou.rs:6                   use cedar_policy
❌ src/shared/domain/scp.rs:5                  use cedar_policy
❌ src/features/create_scp/use_case.rs:7       use cedar_policy::PolicySet
❌ src/features/get_effective_scps/dto.rs:1    use cedar_policy::PolicySet
❌ src/features/get_effective_scps/use_case.rs:7
❌ src/shared/infrastructure/surreal/organization_boundary_provider.rs:8
```

**hodei-iam (2 archivos en infraestructura):**
```
⚠️ src/shared/infrastructure/surreal/iam_policy_provider.rs:8
⚠️ src/shared/infrastructure/surreal/policy_repository.rs:2
```

**Acción Requerida:**
```bash
# Eliminar TODOS los imports de cedar_policy de dominios
# Usar SOLO tipos agnósticos del kernel
```

---

#### ❌ PROBLEMA #5: Evaluadores Autónomos Incompletos

**hodei-organizations:**
- ❌ `features/evaluate_scps/` NO EXISTE
- ⚠️ `create_scp` retorna `PolicySet` (tipo Cedar) en lugar de string
- ⚠️ `get_effective_scps` retorna `PolicySet` (tipo Cedar)

**hodei-iam:**
- ⚠️ `features/evaluate_iam_policies/` existe pero incompleto
- Falta implementar trait `IamPolicyEvaluator` completamente

---

## 🎯 Plan de Acción Corregido (3 Fases)

### 🔥 FASE 1: Limpiar y Preparar Infraestructura (1-2 semanas)

**Objetivo:** Eliminar código incorrecto y preparar base para evaluación agnóstica

#### Tarea 1.1: Limpiar `policies` de features incorrectas ⏰ 1 día ✅ COMPLETADA

**Responsable:** Backend Lead  
**Prioridad:** 🔥 CRÍTICA  
**Estado:** ✅ **COMPLETADA** - 2025-01-XX  
**Commit:** `529c760` - "feat(policies): Eliminar features de gestión - mantener solo evaluación"

**Acciones Realizadas:**
```bash
# 1. ✅ Eliminadas features de gestión
rm -rf crates/policies/src/features/create_policy/
rm -rf crates/policies/src/features/batch_eval/
rm -rf crates/policies/src/features/evaluate_policies/
rm -rf crates/policies/src/features/policy_analysis/
rm -rf crates/policies/src/features/policy_playground/
rm -rf crates/policies/src/features/policy_playground_traces/

# 2. ✅ Movido validate_policy como utilidad
mv features/validate_policy/ shared/infrastructure/validator/

# 3. ✅ Eliminado directorio features completo
rm -rf crates/policies/src/features/
```

**Estructura Final Lograda:**
```
crates/policies/src/
├── lib.rs                    ✅ Actualizado con documentación completa
└── shared/
    ├── application/
    │   ├── engine.rs         ✅ Existe (detrás de legacy_infra flag)
    │   └── di_helpers.rs     ✅ Existe
    └── infrastructure/
        ├── validator/        ✅ Movido desde features/
        └── surreal/          ✅ Legacy code
```

**Resultados:**
- ✅ Eliminados 4,230+ líneas de código de features incorrectas
- ✅ 32 archivos eliminados
- ✅ `cargo check -p policies` compila sin errores
- ✅ Documentación del crate actualizada en `lib.rs`
- ✅ Re-exports configurados correctamente con feature flags

**Verificación:**
```bash
$ ls crates/policies/src/features/
ls: cannot access 'crates/policies/src/features/': No such file or directory
✅ CORRECTO - Directorio features eliminado

$ cargo check -p policies
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.17s
✅ CORRECTO - Compila sin errores
```

**Impacto Arquitectónico:**
- ✅ `policies` es ahora **biblioteca de evaluación pura**
- ✅ Gestión de políticas delegada a dominios (hodei-iam, hodei-organizations)
- ✅ Preparado para recibir el traductor Cedar

---

#### Tarea 1.2: Implementar Traductor Cedar ⏰ 3-4 días

**Responsable:** Senior Backend Engineer  
**Prioridad:** 🔥 BLOQUEANTE CRÍTICO

**Pasos:**

1. **Crear archivo traductor**
   ```bash
   touch crates/policies/src/shared/infrastructure/translator.rs
   ```

2. **Implementar funciones de traducción**
   - `translate_attribute_value()` - 20+ casos de prueba
   - `translate_to_cedar_entity()` - 15+ casos de prueba
   - Manejo de errores explícito

3. **Tests exhaustivos**
   - Primitivos (Bool, Long, String)
   - Colecciones (Set, Record)
   - Estructuras anidadas
   - Entity references
   - Error cases (valores inválidos, HRNs malformados)

**Criterios de Aceptación:**
- [ ] Archivo `translator.rs` existe y compila
- [ ] 30+ tests unitarios pasando
- [ ] Todos los tipos `AttributeValue` se traducen correctamente
- [ ] Manejo robusto de errores
- [ ] Documentación completa con ejemplos

---

#### Tarea 1.3: Refactorizar AuthorizationEngine ⏰ 2-3 días

**Responsable:** Backend Lead  
**Prioridad:** 🔥 CRÍTICA  
**Dependencia:** Requiere Tarea 1.2 completada

**Pasos:**

1. **Definir API pública agnóstica**
   - Crear `EngineRequest` struct (sin tipos Cedar)
   - Crear `EngineResponse` struct (sin tipos Cedar)
   - Crear `EngineError` enum

2. **Refactorizar método `is_authorized()`**
   - Aceptar `EngineRequest` con `&dyn HodeiEntity`
   - Usar traductor internamente
   - Retornar respuesta agnóstica

3. **Métodos auxiliares**
   - `load_policies(Vec<String>)` - cargar DSL Cedar
   - `register_entities(Vec<&dyn HodeiEntity>)` - registrar entidades
   - Builder para configuración inicial

4. **Tests de integración**
   - 20+ tests con entidades reales
   - Casos de Allow/Deny
   - Jerarquías de entidades
   - Context attributes

**Criterios de Aceptación:**
- [ ] API pública NO expone tipos Cedar
- [ ] Tests de evaluación pasan con tipos agnósticos
- [ ] Performance no se degrada (benchmark)
- [ ] Documentación con ejemplos

---

#### Tarea 1.4: Limpiar Cedar de Entidades de Dominio ⏰ 2-3 días

**Responsable:** Domain Experts  
**Prioridad:** 🔥 ALTA

**hodei-organizations:**
```bash
# Eliminar imports Cedar de dominio
# account.rs, ou.rs, scp.rs
# Usar SOLO kernel::HodeiEntity, kernel::AttributeValue

# Refactorizar features
# create_scp: Retornar String (Cedar DSL), NO PolicySet
# get_effective_scps: Retornar Vec<String>, NO PolicySet
```

**hodei-iam:**
```bash
# Eliminar imports Cedar de infraestructura
# iam_policy_provider.rs
# policy_repository.rs
# Usar SOLO strings para almacenar políticas
```

**Criterios de Aceptación:**
- [ ] `grep -r "use cedar_policy" crates/hodei-*/src/shared/domain/` → 0 matches
- [ ] `cargo check -p hodei-iam` → Success
- [ ] `cargo check -p hodei-organizations` → Success
- [ ] Tests de dominio pasan sin Cedar

---

#### Tarea 1.5: Sellar Bounded Contexts ⏰ 1 día

**Responsable:** Tech Lead  
**Prioridad:** 🟡 MEDIA

**Cambio Simple:**
```rust
// hodei-iam/src/lib.rs
// ANTES:
pub mod shared;  // ❌

// DESPUÉS:
mod shared;      // ✅ Encapsulación forzada

// hodei-organizations/src/lib.rs
mod shared;      // ✅
```

**Criterios de Aceptación:**
- [ ] Módulos `shared` son privados
- [ ] Solo se exponen APIs públicas necesarias
- [ ] Compilación exitosa

---

### 🚀 FASE 2: Implementar Evaluadores Autónomos (2-3 semanas)

**Objetivo:** Cada dominio evalúa sus propias políticas de forma completamente autónoma

#### Tarea 2.1: Implementar EvaluateScpsUseCase (hodei-organizations) ⏰ 4-5 días

**Responsable:** Organizations Team  
**Prioridad:** 🔥 CRÍTICA  
**Dependencia:** Fase 1 completada

**Estructura a Crear:**
```
crates/hodei-organizations/src/features/evaluate_scps/
├── mod.rs
├── use_case.rs              # Implementa ScpEvaluator trait
├── ports.rs                 # ScpRepository, OuHierarchyProvider
├── adapter.rs               # Implementaciones concretas
├── dto.rs                   # Request/Response types
├── error.rs                 # ScpEvaluationError
├── di.rs                    # Dependency injection
├── mocks.rs                 # Para tests
└── use_case_test.rs         # 30+ tests unitarios
```

**Implementación del Use Case:**

```rust
// use_case.rs
use kernel::application::ports::authorization::{
    ScpEvaluator, EvaluationRequest, EvaluationDecision, AuthorizationError
};
use policies::shared::application::engine::AuthorizationEngine;
use std::sync::Arc;

pub struct EvaluateScpsUseCase<SR, HR> {
    scp_repository: SR,
    hierarchy_provider: HR,
    engine: Arc<AuthorizationEngine>,
}

impl<SR, HR> EvaluateScpsUseCase<SR, HR>
where
    SR: ScpRepository,
    HR: OuHierarchyProvider,
{
    pub fn new(
        scp_repository: SR,
        hierarchy_provider: HR,
        engine: Arc<AuthorizationEngine>,
    ) -> Self {
        Self { scp_repository, hierarchy_provider, engine }
    }
}

impl<SR, HR> ScpEvaluator for EvaluateScpsUseCase<SR, HR>
where
    SR: ScpRepository + Send + Sync,
    HR: OuHierarchyProvider + Send + Sync,
{
    fn evaluate_scps(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        // 1. Obtener jerarquía OU del resource
        let hierarchy = self.hierarchy_provider
            .get_hierarchy(&request.resource_hrn)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 2. Recolectar SCPs efectivas (desde root hasta resource)
        let scps = self.scp_repository
            .get_effective_scps_for_hierarchy(&hierarchy)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 3. Cargar SCPs en el engine
        let policy_texts: Vec<String> = scps.iter().map(|scp| scp.content.clone()).collect();
        // Note: Engine debe ser mutable o tener método thread-safe
        // Esto puede requerir refactor del engine
        
        // 4. Construir entidades agnósticas para evaluación
        // (principal, action, resource)
        
        // 5. Evaluar con engine
        let engine_request = policies::shared::application::engine::EngineRequest {
            principal: &principal_entity,  // Debe implementar HodeiEntity
            action: &request.action_name,
            resource: &resource_entity,    // Debe implementar HodeiEntity
            context: std::collections::HashMap::new(),
        };
        
        let allowed = self.engine.is_authorized(engine_request)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 6. Retornar decisión
        Ok(EvaluationDecision {
            principal_hrn: request.principal_hrn,
            action_name: request.action_name,
            resource_hrn: request.resource_hrn,
            decision: allowed,
            reason: if allowed { "Allowed by SCP".to_string() } else { "Denied by SCP".to_string() },
        })
    }
}
```

**Criterios de Aceptación:**
- [ ] Implementa trait `ScpEvaluator` completamente
- [ ] Resuelve jerarquía OU correctamente
- [ ] Integra con `AuthorizationEngine` vía traductor
- [ ] 30+ tests unitarios con mocks
- [ ] Manejo robusto de errores
- [ ] Logging con `tracing`

---

#### Tarea 2.2: Implementar EvaluateIamPoliciesUseCase (hodei-iam) ⏰ 4-5 días

**Responsable:** IAM Team  
**Prioridad:** 🔥 CRÍTICA  
**Dependencia:** Fase 1 completada

**Estructura a Completar/Crear:**
```
crates/hodei-iam/src/features/evaluate_iam_policies/
├── mod.rs               ✅ Ya existe
├── use_case.rs          ⚠️ Completar implementación
├── ports.rs             ✅ Ya existe
├── adapter.rs           ⚠️ Revisar/completar
├── dto.rs               ❌ Crear
├── error.rs             ❌ Crear
├── di.rs                ⚠️ Revisar
├── mocks.rs             ⚠️ Revisar
└── use_case_test.rs     ⚠️ Aumentar cobertura
```

**Implementación:**

```rust
// use_case.rs
use kernel::application::ports::authorization::{
    IamPolicyEvaluator, EvaluationRequest, EvaluationDecision, AuthorizationError
};
use policies::shared::application::engine::AuthorizationEngine;
use std::sync::Arc;

pub struct EvaluateIamPoliciesUseCase<PR, UR, GR> {
    policy_repository: PR,
    user_repository: UR,
    group_repository: GR,
    engine: Arc<AuthorizationEngine>,
}

impl<PR, UR, GR> IamPolicyEvaluator for EvaluateIamPoliciesUseCase<PR, UR, GR>
where
    PR: IamPolicyRepository + Send + Sync,
    UR: UserRepository + Send + Sync,
    GR: GroupRepository + Send + Sync,
{
    fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        // 1. Resolver principal (User + sus Groups)
        let user = self.user_repository
            .get_by_hrn(&request.principal_hrn)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        let groups = self.group_repository
            .get_groups_for_user(&user.hrn)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 2. Obtener políticas efectivas del principal
        // (políticas directas del user + políticas de sus groups)
        let policies = self.policy_repository
            .get_effective_policies_for_principal(&request.principal_hrn)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 3. Cargar políticas en engine
        let policy_texts: Vec<String> = policies.iter().map(|p| p.content.clone()).collect();
        
        // 4. Construir request agnóstico con entidades
        let engine_request = policies::shared::application::engine::EngineRequest {
            principal: &user,  // User implementa HodeiEntity
            action: &request.action_name,
            resource: &resource_entity,
            context: std::collections::HashMap::new(),
        };
        
        // 5. Evaluar
        let allowed = self.engine.is_authorized(engine_request)
            .map_err(|e| AuthorizationError::EvaluationFailed(e.to_string()))?;
        
        // 6. Retornar decisión
        Ok(EvaluationDecision {
            principal_hrn: request.principal_hrn,
            action_name: request.action_name,
            resource_hrn: request.resource_hrn,
            decision: allowed,
            reason: if allowed { "Allowed by IAM policy".to_string() } else { "Denied by IAM policy".to_string() },
        })
    }
}
```

**Criterios de Aceptación:**
- [ ] Implementa trait `IamPolicyEvaluator` completamente
- [ ] Resuelve principal + grupos correctamente
- [ ] Integra con `AuthorizationEngine`
- [ ] 30+ tests unitarios
- [ ] Manejo de errores robusto

---

#### Tarea 2.3: Eliminar Código Obsoleto ⏰ 1-2 días

**Responsable:** Tech Lead  
**Prioridad:** 🟡 MEDIA

**Archivos a Eliminar:**

**hodei-organizations:**
```bash
rm crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs
rm crates/hodei-organizations/src/shared/application/hierarchy_service.rs  # Si no se usa
```

**hodei-iam:**
```bash
rm crates/hodei-iam/src/shared/infrastructure/surreal/iam_policy_provider.rs
```

**hodei-authorizer:**
```bash
# Revisar si authorizer.rs es obsoleto
# Si tiene lógica legacy, eliminar
```

**Criterios de Aceptación:**
- [ ] Archivos obsoletos eliminados
- [ ] No hay referencias rotas
- [ ] Tests pasan
- [ ] Compilación exitosa

---

### 🏗️ FASE 3: Componer Aplicación Monolítica (1-2 semanas)

**Objetivo:** Ensamblar todos los componentes en el API monolítico funcional

#### Tarea 3.1: Implementar Composition Root ⏰ 3-4 días

**Responsable:** Platform Team  
**Prioridad:** 🟡 ALTA  
**Dependencia:** Fase 2 completada

**Ubicación:** `src/lib.rs` o `src/composition_root.rs`

**Implementación:**

```rust
// src/composition_root.rs
use std::sync::Arc;
use anyhow::Result;

pub struct AppState {
    // Authorizer (orquestador)
    pub authorizer: Arc<hodei_authorizer::EvaluatePermissionsUseCase<...>>,
    
    // IAM Management
    pub create_user: Arc<hodei_iam::CreateUserUseCase<...>>,
    pub create_group: Arc<hodei_iam::CreateGroupUseCase<...>>,
    pub add_user_to_group: Arc<hodei_iam::AddUserToGroupUseCase<...>>,
    pub create_iam_policy: Arc<hodei_iam::CreatePolicyUseCase<...>>,
    
    // Organizations Management
    pub create_account: Arc<hodei_organizations::CreateAccountUseCase<...>>,
    pub create_ou: Arc<hodei_organizations::CreateOuUseCase<...>>,
    pub create_scp: Arc<hodei_organizations::CreateScpUseCase<...>>,
    pub attach_scp: Arc<hodei_organizations::AttachScpUseCase<...>>,
}

pub async fn build_app_state(config: &Config) -> Result<AppState> {
    // 1. Conectar a base de datos
    let db = surrealdb::Surreal::new::<surrealdb::engine::any::Any>(&config.db_url).await?;
    db.use_ns("hodei").use_db("artifacts").await?;
    
    // 2. Instanciar repositorios
    let user_repo = Arc::new(hodei_iam::SurrealUserRepository::new(db.clone()));
    let group_repo = Arc::new(hodei_iam::SurrealGroupRepository::new(db.clone()));
    let policy_repo = Arc::new(hodei_iam::SurrealPolicyRepository::new(db.clone()));
    
    let account_repo = Arc::new(hodei_organizations::SurrealAccountRepository::new(db.clone()));
    let ou_repo = Arc::new(hodei_organizations::SurrealOuRepository::new(db.clone()));
    let scp_repo = Arc::new(hodei_organizations::SurrealScpRepository::new(db.clone()));
    
    // 3. Construir AuthorizationEngines
    // Uno para IAM, otro para Organizations (o compartir si es apropiado)
    let iam_engine = Arc::new(policies::AuthorizationEngine::new(/* schema */));
    let org_engine = Arc::new(policies::AuthorizationEngine::new(/* schema */));
    
    // 4. Construir evaluadores
    let iam_evaluator: Arc<dyn kernel::IamPolicyEvaluator> = Arc::new(
        hodei_iam::EvaluateIamPoliciesUseCase::new(
            policy_repo.clone(),
            user_repo.clone(),
            group_repo.clone(),
            iam_engine.clone(),
        )
    );
    
    let scp_evaluator: Arc<dyn kernel::ScpEvaluator> = Arc::new(
        hodei_organizations::EvaluateScpsUseCase::new(
            scp_repo.clone(),
            ou_repo.clone(),
            org_engine.clone(),
        )
    );
    
    // 5. Construir authorizer (orquestador)
    let cache = Some(RedisCache::new(config.redis_url.clone()));
    let logger = AuditLogger::new(db.clone());
    let metrics = PrometheusMetrics::new();
    
    let authorizer = Arc::new(
        hodei_authorizer::EvaluatePermissionsUseCase::new(
            iam_evaluator,
            scp_evaluator,
            cache,
            logger,
            metrics,
        )
    );
    
    // 6. Construir casos de uso de gestión
    let create_user = Arc::new(hodei_iam::CreateUserUseCase::new(user_repo.clone()));
    let create_group = Arc::new(hodei_iam::CreateGroupUseCase::new(group_repo.clone()));
    // ... más casos de uso
    
    // 7. Retornar AppState
    Ok(AppState {
        authorizer,
        create_user,
        create_group,
        // ... resto
    })
}
```

**Criterios de Aceptación:**
- [ ] Función `build_app_state()` completa
- [ ] Todos los casos de uso correctamente cableados
- [ ] Inyección de dependencias clara
- [ ] Aplicación arranca correctamente
- [ ] Tests de integración E2E pasan

---

#### Tarea 3.2: Reorganizar API Handlers ⏰ 2-3 días

**Responsable:** API Team  
**Prioridad:** 🟡 ALTA

**Estructura Objetivo:**

```
src/api/
├── mod.rs
├── authorization.rs         # POST /authorize
├── iam/
│   ├── mod.rs
│   ├── users.rs             # CRUD /iam/users
│   ├── groups.rs            # CRUD /iam/groups
│   └── policies.rs          # CRUD /iam/policies
└── organizations/
    ├── mod.rs
    ├── accounts.rs          # CRUD /organizations/accounts
    ├── ous.rs               # CRUD /organizations/ous
    └── scps.rs              # CRUD /organizations/scps
```

**Patrón de Handler:**

```rust
// src/api/authorization.rs
use axum::{Json, Extension};
use crate::AppState;

#[derive(serde::Deserialize)]
pub struct AuthorizeRequest {
    pub principal: String,
    pub action: String,
    pub resource: String,
}

pub async fn authorize_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<AuthorizeRequest>,
) -> Result<Json<AuthorizeResponse>, ApiError> {
    // 1. Convertir HTTP request a DTO del caso de uso
    let auth_request = hodei_authorizer::AuthorizationRequest {
        principal: kernel::Hrn::from_str(&request.principal)?,
        action: request.action,
        resource: kernel::Hrn::from_str(&request.resource)?,
    };
    
    // 2. Llamar al caso de uso
    let response = state.authorizer.execute(auth_request).await?;
    
    // 3. Convertir respuesta a HTTP
    Ok(Json(AuthorizeResponse {
        allowed: matches!(response.decision, AuthorizationDecision::Allow),
        reason: response.reason,
    }))
}
```

**Criterios de Aceptación:**
- [ ] Handlers solo tienen lógica HTTP (sin lógica de negocio)
- [ ] Todos los endpoints funcionan
- [ ] Tests E2E pasan
- [ ] Documentación OpenAPI actualizada

---

#### Tarea 3.3: Tests de Integración E2E ⏰ 2-3 días

**Responsable:** QA + Backend  
**Prioridad:** 🟢 ALTA

**Cobertura Requerida:**

1. **Flujo completo de autorización:**
   - Crear user, group, policy
   - Evaluar permiso (debe denegar)
   - Adjuntar policy a user
   - Evaluar permiso (debe permitir)

2. **Flujo SCP:**
   - Crear account, OU, SCP
   - Adjuntar SCP a OU
   - Evaluar permiso en account dentro de OU
   - Verificar deny de SCP

3. **Orquestación SCP + IAM:**
   - SCP permite, IAM permite → Allow
   - SCP permite, IAM deniega → Deny
   - SCP deniega, IAM permite → Deny

**Criterios de Aceptación:**
- [ ] 20+ tests E2E pasando
- [ ] Cobertura de casos críticos
- [ ] Tests usan testcontainers (aislados)
- [ ] Ejecutan en < 30 segundos

---

## 📊 Métricas de Éxito

### Objetivos Finales

| Métrica | Objetivo | Verificación |
|---------|----------|--------------|
| **Compilación** | 5/5 crates sin errores | `cargo check --workspace` |
| **Tests** | 200+ tests pasando | `cargo test --workspace` |
| **Cobertura** | 80%+ de líneas | `cargo tarpaulin` |
| **Clippy** | 0 warnings | `cargo clippy -- -D warnings` |
| **Cedar en dominio** | 0 archivos | `grep -r "use cedar_policy" crates/*/src/shared/domain/` |
| **Features VSA** | 15+ features completas | Revisión manual |
| **Arquitectura** | 100% agnóstica | Revisión de imports |

### Checklist de Calidad por Fase

**Fase 1:**
- [✅] `policies` sin features de gestión (Tarea 1.1 COMPLETADA)
- [ ] Traductor implementado con 30+ tests (Tarea 1.2 EN PROGRESO)
- [ ] `AuthorizationEngine` con API agnóstica (Tarea 1.3 PENDIENTE)
- [ ] Cero imports Cedar en dominios (Tarea 1.4 PENDIENTE)
- [ ] Bounded contexts sellados (Tarea 1.5 PENDIENTE)

**Fase 2:**
- [ ] `EvaluateScpsUseCase` implementado
- [ ] `EvaluateIamPoliciesUseCase` implementado
- [ ] Código obsoleto eliminado
- [ ] 60+ tests nuevos pasando

**Fase 3:**
- [ ] `AppState` compone correctamente
- [ ] API handlers reorganizados
- [ ] 20+ tests E2E pasando
- [ ] Documentación actualizada

---

## 🚧 Riesgos y Mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Performance del traductor | Media | Alto | Benchmark temprano, caché si necesario |
| Complejidad jerarquía OU | Alta | Medio | Tests exhaustivos, algoritmo bien documentado |
| Engine thread-safety | Media | Alto | Revisar diseño, considerar Arc<RwLock<>> |
| Resistencia a eliminar legacy | Baja | Medio | Feature flags temporales, comunicación clara |
| Tiempo de migración | Alta | Alto | Fases incrementales, rollback posible |

---

## 📅 Timeline Estimado

```
Semana 1:     [████░░░░░░░░░░░░░░░░] Fase 1 - Limpieza (40%)
Semana 2:     [████████░░░░░░░░░░░░] Fase 1 - Traductor (60%)
Semana 3:     [████████████░░░░░░░░] Fase 2 - Evaluadores (70%)
Semana 4-5:   [████████████████░░░░] Fase 2 - Completar (80%)
Semana 6:     [████████████████████] Fase 3 - Composición (90%)
Semana 7:     [████████████████████] Tests E2E y Polish (100%)
```

**ETA:** 7-8 semanas para sistema 100% operacional

---

## 📚 Referencias

- [Historias de Usuario Originales](./historias-usuario.md)
- [Cedar Policy Documentation](https://www.cedarpolicy.com/)
- [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)
- [Vertical Slice Architecture](https://www.jimmybogard.com/vertical-slice-architecture/)

---

**Última Actualización:** 2025-01-XX  
**Responsable:** Tech Lead  
**Estado:** 🔴 Acción Inmediata Requerida

---

## ✅ Próximos Pasos Inmediatos

1. ✅ **COMPLETADO:** Tarea 1.1 - Limpieza de `policies` (features eliminadas)
2. **EN PROGRESO:** Tarea 1.2 - Implementar traductor Cedar
3. **HOY:** Continuar con implementación del traductor
4. **Esta semana:** Completar traductor y comenzar refactor de AuthorizationEngine