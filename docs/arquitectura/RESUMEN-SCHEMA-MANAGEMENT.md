# Resumen Ejecutivo: Schema Management y Próximos Pasos

## 🎯 Problema Identificado

Actualmente, `hodei-iam` usa directamente la implementación `EvaluatePoliciesUseCase` de `hodei-policies`, violando los principios de arquitectura:

❌ **INCORRECTO (estado actual)**:
```rust
use hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase;

// Uso directo de implementación
let evaluator = EvaluatePoliciesUseCase::new();
```

✅ **CORRECTO (debe ser)**:
```rust
use hodei_policies::features::evaluate_policies::EvaluatePoliciesPort;

// Uso de port (trait) - implementación inyectada por DI
let evaluator: Arc<dyn EvaluatePoliciesPort> = /* inyectado */;
```

## 🏗️ Arquitectura Requerida

### Principio Fundamental
**Todo debe exponerse a través de PORTS (traits) y USE CASES, NUNCA implementaciones directas**

### Flujo de Schema Dinámico

```
[Arranque de Aplicación]
         ↓
[Inicializar SchemaBuilder Singleton]
         ↓
[Cada BC registra sus tipos]
    - hodei-iam → User, Group, CreateUser, DeleteUser, etc.
    - hodei-orgs → Organization, SCP, etc.
    - hodei-artifacts → Artifact, Version, etc.
         ↓
[Construir Schema Final Cedar]
         ↓
[Comparar con Schema Persistido]
         ↓
    ¿Cambió?
    ├─ SI → Guardar nuevo schema en SurrealDB
    └─ NO → Usar schema existente
         ↓
[Aplicación lista para evaluar políticas]
```

## 📋 Lista de Tareas Prioritarias

### 🔴 FASE 1: Refactorización Crítica (URGENTE)

#### 1.1. hodei-policies: Crear Port para Evaluación

**Archivos a crear/modificar:**

- `crates/hodei-policies/src/features/evaluate_policies/ports.rs` (NUEVO)
  ```rust
  #[async_trait]
  pub trait EvaluatePoliciesPort: Send + Sync {
      async fn evaluate(
          &self,
          command: EvaluatePoliciesCommand<'_>,
      ) -> Result<EvaluationDecision, EvaluatePoliciesError>;
  }
  ```

- `crates/hodei-policies/src/features/evaluate_policies/use_case.rs` (MODIFICAR)
  ```rust
  // Implementar el port
  #[async_trait]
  impl EvaluatePoliciesPort for EvaluatePoliciesUseCase {
      async fn evaluate(&self, command: ...) -> Result<...> {
          self.execute(command).await
      }
  }
  ```

- `crates/hodei-policies/src/features/evaluate_policies/mod.rs` (MODIFICAR)
  ```rust
  pub mod ports;
  pub use ports::EvaluatePoliciesPort;
  ```

**Tiempo estimado:** 30 minutos

---

#### 1.2. hodei-iam: Actualizar evaluate_iam_policies para usar Port

**Archivos a modificar:**

- `crates/hodei-iam/src/features/evaluate_iam_policies/ports.rs`
  ```rust
  // Re-exportar port de hodei-policies
  pub use hodei_policies::features::evaluate_policies::EvaluatePoliciesPort;
  ```

- `crates/hodei-iam/src/features/evaluate_iam_policies/use_case.rs`
  ```rust
  // Cambiar de usar EvaluatePoliciesUseCase a EvaluatePoliciesPort
  pub struct EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
  where
      EP: EvaluatePoliciesPort,  // ← Cambio clave
  {
      policies_evaluator: Arc<EP>,  // ← Ahora es Arc del port
  }
  ```

- `crates/hodei-iam/src/features/evaluate_iam_policies/di.rs`
  ```rust
  // Actualizar factory para aceptar el port
  pub fn build<PF, PR, RR, EP>(
      policies_evaluator: Arc<EP>,  // ← Inyectar port
  ) -> EvaluateIamPoliciesUseCase<PF, PR, RR, EP>
  ```

- `crates/hodei-iam/src/features/evaluate_iam_policies/use_case_test.rs`
  ```rust
  // Actualizar mocks para implementar EvaluatePoliciesPort
  ```

**Tiempo estimado:** 1 hora

**Criterios de aceptación:**
- ✅ Código compila sin errores
- ✅ No hay warnings de clippy
- ✅ Todos los tests pasan
- ✅ hodei-iam NO importa EvaluatePoliciesUseCase directamente

---

### 🟡 FASE 2: Features de Schema Registration (IMPORTANTE)

#### 2.1. hodei-policies: Feature `register_entity_type`

**Estructura completa VSA:**
```
crates/hodei-policies/src/features/register_entity_type/
├── mod.rs
├── use_case.rs          # RegisterEntityTypeUseCase
├── ports.rs             # SchemaBuilderPort
├── dto.rs               # RegisterEntityTypeCommand, EntityTypeView
├── error.rs             # RegisterEntityTypeError
├── di.rs                # Factory
├── mocks.rs             # Mocks para tests
└── use_case_test.rs     # Tests unitarios
```

**Responsabilidad:** Registrar entity types (User, Group, etc.) en el SchemaBuilder

**Tiempo estimado:** 2 horas

---

#### 2.2. hodei-policies: Feature `register_action_type`

**Estructura completa VSA:**
```
crates/hodei-policies/src/features/register_action_type/
├── mod.rs
├── use_case.rs          # RegisterActionTypeUseCase
├── ports.rs             # ActionRegistrarPort
├── dto.rs               # RegisterActionTypeCommand, ActionTypeView
├── error.rs             # RegisterActionTypeError
├── di.rs                # Factory
├── mocks.rs
└── use_case_test.rs
```

**Responsabilidad:** Registrar action types (CreateUser, DeleteUser, etc.) en el SchemaBuilder

**Tiempo estimado:** 2 horas

---

#### 2.3. hodei-policies: Feature `build_schema`

**Estructura completa VSA:**
```
crates/hodei-policies/src/features/build_schema/
├── mod.rs
├── use_case.rs          # BuildSchemaUseCase
├── ports.rs             # SchemaGeneratorPort
├── dto.rs               # BuildSchemaCommand, SchemaView
├── error.rs             # BuildSchemaError
├── di.rs
├── mocks.rs
└── use_case_test.rs
```

**Responsabilidad:** Generar el schema final de Cedar desde tipos registrados

**Tiempo estimado:** 2 horas

---

#### 2.4. hodei-policies: Feature `persist_schema`

**Estructura completa VSA:**
```
crates/hodei-policies/src/features/persist_schema/
├── mod.rs
├── use_case.rs          # PersistSchemaUseCase
├── ports.rs             # SchemaStoragePort
├── dto.rs               # PersistSchemaCommand, PersistedSchemaView
├── error.rs             # PersistSchemaError
├── di.rs
├── mocks.rs
└── use_case_test.rs
```

**Responsabilidad:** Guardar schema en SurrealDB (solo si cambió)

**Tiempo estimado:** 2 horas

---

#### 2.5. hodei-policies: Adaptador SurrealDB

**Archivo:**
```
crates/hodei-policies/src/infrastructure/surreal/schema_storage_adapter.rs
```

**Responsabilidad:** Implementar `SchemaStoragePort` con SurrealDB

**Tabla SurrealDB:**
```sql
DEFINE TABLE cedar_schemas SCHEMAFULL;
DEFINE FIELD schema_content ON cedar_schemas TYPE string;
DEFINE FIELD schema_hash ON cedar_schemas TYPE string;
DEFINE FIELD version ON cedar_schemas TYPE string;
DEFINE FIELD created_at ON cedar_schemas TYPE datetime;
DEFINE INDEX schema_hash_idx ON cedar_schemas FIELDS schema_hash;
```

**Tiempo estimado:** 1.5 horas

---

### 🟢 FASE 3: hodei-iam Schema Registration (NORMAL)

#### 3.1. Crear módulo `internal/actions/`

**Estructura:**
```
crates/hodei-iam/src/internal/actions/
├── mod.rs
├── create_user_action.rs
├── update_user_action.rs
├── delete_user_action.rs
├── create_group_action.rs
├── update_group_action.rs
├── delete_group_action.rs
├── add_to_group_action.rs
└── remove_from_group_action.rs
```

**Cada action implementa `kernel::ActionTrait`:**
```rust
pub struct CreateUserAction;

impl ActionTrait for CreateUserAction {
    fn name() -> &'static str { "CreateUser" }
    fn service_name() -> ServiceName { ServiceName::new("iam").unwrap() }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Account".to_string() }
}
```

**Tiempo estimado:** 2 horas

---

#### 3.2. Feature `register_iam_schema`

**Estructura completa VSA:**
```
crates/hodei-iam/src/features/register_iam_schema/
├── mod.rs
├── use_case.rs          # RegisterIamSchemaUseCase
├── ports.rs             # RegisterEntityTypePort, RegisterActionTypePort
├── dto.rs               # RegisterIamSchemaCommand, IamSchemaView
├── error.rs             # RegisterIamSchemaError
├── di.rs
└── use_case_test.rs
```

**Responsabilidad:** Registrar todos los tipos IAM al arranque
- Entities: User, Group, Account
- Actions: CreateUser, DeleteUser, AddToGroup, etc.

**Tiempo estimado:** 2.5 horas

---

### 🔵 FASE 4: Integración en main.rs (NORMAL)

#### 4.1. Flujo de Arranque

**Archivo:** `src/main.rs`

**Pseudocódigo:**
```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 1. Inicializar infraestructura
    let db = init_db().await?;
    
    // 2. Crear SchemaBuilder (singleton)
    let schema_builder = Arc::new(SchemaBuilderAdapter::new());
    
    // 3. Registrar schemas de todos los BCs
    register_iam_schema(schema_builder.clone()).await?;
    register_org_schema(schema_builder.clone()).await?;
    register_artifact_schema(schema_builder.clone()).await?;
    
    // 4. Construir schema final
    let schema = build_schema(schema_builder.clone()).await?;
    
    // 5. Persistir si cambió
    persist_schema_if_changed(db.clone(), schema).await?;
    
    // 6. Inicializar app state
    let app_state = init_app_state(db, schema_builder).await?;
    
    // 7. Arrancar servidor
    start_server(app_state).await?;
    
    Ok(())
}
```

**Tiempo estimado:** 3 horas

---

## 📊 Resumen de Tiempo

| Fase | Descripción | Tiempo Estimado |
|------|-------------|-----------------|
| 1.1  | hodei-policies: Port evaluación | 30 min |
| 1.2  | hodei-iam: Usar port | 1 hora |
| 2.1  | register_entity_type | 2 horas |
| 2.2  | register_action_type | 2 horas |
| 2.3  | build_schema | 2 horas |
| 2.4  | persist_schema | 2 horas |
| 2.5  | Adaptador SurrealDB | 1.5 horas |
| 3.1  | Actions IAM | 2 horas |
| 3.2  | register_iam_schema | 2.5 horas |
| 4.1  | Integración main.rs | 3 horas |
| **TOTAL** | | **~18.5 horas** |

## ✅ Criterios de Aceptación Global

Al completar todas las fases:

- [ ] hodei-iam NO importa implementaciones de hodei-policies
- [ ] Todo se expone via features VSA y ports
- [ ] Schema se construye dinámicamente al arranque
- [ ] Schema se persiste en SurrealDB
- [ ] Schema se reutiliza si no cambió
- [ ] Todos los bounded contexts registran sus tipos
- [ ] 100% de tests unitarios pasan
- [ ] 0 warnings de clippy
- [ ] Código compila sin errores
- [ ] Arquitectura VSA estricta respetada
- [ ] ISP respetado en todos los ports
- [ ] Cero acoplamiento entre bounded contexts

## 🚀 Orden de Implementación Recomendado

1. **PRIMERO**: Fase 1.1 y 1.2 (refactorización crítica)
2. **SEGUNDO**: Fase 2.1, 2.2, 2.3, 2.4, 2.5 (features schema en hodei-policies)
3. **TERCERO**: Fase 3.1 y 3.2 (schema registration en hodei-iam)
4. **CUARTO**: Fase 4.1 (integración en main.rs)

## 📚 Referencias

- Documento completo: `docs/arquitectura/schema-management-architecture.md`
- Plan hodei-iam: `docs/historias/hodei-iam-plan.md`
- Reglas arquitectura: `AGENTS.md`
- Solución schema: `SCHEMA_SOLUTION.md`

## 🎯 Próximo Paso Inmediato

**Empezar con Fase 1.1**: Crear `EvaluatePoliciesPort` en hodei-policies

```bash
# 1. Crear archivo de port
touch crates/hodei-policies/src/features/evaluate_policies/ports.rs

# 2. Implementar trait EvaluatePoliciesPort
# 3. Implementar trait en EvaluatePoliciesUseCase
# 4. Exportar en mod.rs
# 5. Ejecutar tests: cargo nextest run --package hodei-policies
```

---

**Estado Actual**: ✅ Planificación completa  
**Siguiente**: 🔴 Implementación Fase 1.1  
**Estimación**: 30 minutos hasta tener el port funcionando