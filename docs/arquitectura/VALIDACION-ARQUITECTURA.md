# Validación de Arquitectura: Schema Management

## ✅ Verificación de Requisitos del Usuario

### Requisito 1: "SchemaBuilderPort debería registrar directamente un tipo ActionTrait, Principal, o Resource"

**Estado:** ✅ **CUMPLIDO** (pero sin necesidad de Port)

**Explicación:**
- El `EngineBuilder` ya existente registra directamente tipos genéricos:
  ```rust
  pub fn register_entity<T: HodeiEntityType>(&mut self) -> Result<...>
  pub fn register_action_type<A: ActionTrait>(&mut self) -> Result<...>
  ```
- No necesitamos un "Port" porque el registro es **síncrono** (operaciones en memoria)
- El `EngineBuilder` se pasa directamente por referencia mutable: `&mut EngineBuilder`

**Ejemplo de uso:**
```rust
let mut builder = EngineBuilder::new();

// Registra User (implementa HodeiEntityType)
builder.register_entity::<User>()?;

// Registra CreateUserAction (implementa ActionTrait)
builder.register_action_type::<CreateUserAction>()?;
```

**¿Por qué no necesita Port?**
- El registro es síncrono (no async)
- No accede a recursos externos (solo memoria)
- El `EngineBuilder` es suficiente como abstracción

---

### Requisito 2: "Principal y Resource son lo que usa Cedar para su schema"

**Estado:** ✅ **CUMPLIDO**

**Explicación:**
- `Principal` y `Resource` son marker traits del kernel:
  ```rust
  pub trait Principal: HodeiEntity + HodeiEntityType {}
  pub trait Resource: HodeiEntity + HodeiEntityType {}
  ```
- Ambos heredan de `HodeiEntityType`, que es lo que Cedar necesita
- El `EngineBuilder` usa `HodeiEntityType` para generar el schema
- Los tipos que implementan `Principal` o `Resource` automáticamente tienen acceso a `HodeiEntityType`

**Ejemplo:**
```rust
// User implementa Principal
impl Principal for User {}

// Document implementa Resource
impl Resource for Document {}

// Ambos se registran igual (usan HodeiEntityType)
builder.register_entity::<User>()?;      // Principal
builder.register_entity::<Document>()?;  // Resource
```

---

### Requisito 3: "Cada implementación extrae los schema fragments como tenemos especificado e implementado"

**Estado:** ✅ **CUMPLIDO**

**Explicación:**
- El `EngineBuilder` YA tiene implementadas las funciones de extracción:
  ```rust
  fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment>
  fn generate_action_fragment<A: ActionTrait>() -> Result<SchemaFragment>
  ```
- Estas funciones extraen automáticamente:
  - Nombre del servicio: `T::service_name()`
  - Nombre del tipo: `T::resource_type_name()` 
  - Atributos: `T::attributes_schema()`
  - Para actions: `A::applies_to_principal()`, `A::applies_to_resource()`

**Ejemplo de schema fragment generado:**
```cedar
namespace Iam {
    entity User {
        name: String,
        email: String,
        active: Bool,
    }
}

action "CreateUser" appliesTo {
    principal: [Iam::User],
    resource: [Iam::Account]
};
```

---

### Requisito 4: "Persistir el schema puede ser interno"

**Estado:** ✅ **CUMPLIDO**

**Explicación:**
- La feature `build_and_persist_schema` hace ambas cosas automáticamente:
  1. Construir el schema: `builder.build_schema()`
  2. Persistir si cambió: `storage.save_schema()` (interno al use case)

**Flujo interno:**
```rust
pub async fn execute(
    &self,
    builder: EngineBuilder,
) -> Result<SchemaView, ...> {
    // 1. Construir schema
    let schema = builder.build_schema()?;
    
    // 2. Calcular hash
    let hash = calculate_hash(&schema);
    
    // 3. Comparar con existente
    let existing = self.storage.load_latest_schema().await?;
    
    // 4. Persistir solo si cambió (INTERNO)
    if existing.hash != hash {
        self.storage.save_schema(schema, hash).await?;
    }
    
    // 5. Retornar schema listo
    Ok(SchemaView { ... })
}
```

**No se expone como feature separada** - Es parte del `build_and_persist_schema` use case.

---

### Requisito 5: "Esa persistencia recupere el schema para los casos de uso existentes de validación y evaluación"

**Estado:** ✅ **CUMPLIDO**

**Explicación:**
- Feature `load_schema` carga el schema persistido
- Los use cases `validate_policy` y `evaluate_policies` lo usan:

```rust
// validate_policy use case
pub struct ValidatePolicyUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    schema_loader: Arc<SL>,  // ← Port para cargar schema
}

impl<SL> ValidatePolicyUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    pub async fn execute(&self, command: ValidatePolicyCommand) 
        -> Result<ValidationResult, ...> 
    {
        // 1. Cargar schema persistido
        let schema = self.schema_loader.load_schema().await?;
        
        // 2. Validar política contra schema
        let policy = PolicySet::from_str(&command.content)?;
        let validator = Validator::new(schema);
        let result = validator.validate(&policy, ValidationMode::default());
        
        // ...
    }
}
```

**Mismo patrón para `evaluate_policies`:**
```rust
pub struct EvaluatePoliciesUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    schema_loader: Arc<SL>,
}

impl<SL> EvaluatePoliciesUseCase<SL>
where
    SL: SchemaLoaderPort,
{
    pub async fn execute(&self, command: EvaluatePoliciesCommand) 
        -> Result<EvaluationDecision, ...> 
    {
        // 1. Cargar schema
        let schema = self.schema_loader.load_schema().await?;
        
        // 2. Crear authorizer con schema
        let authorizer = Authorizer::new();
        
        // 3. Evaluar con schema validation
        let decision = authorizer.is_authorized(
            &request,
            &policies,
            &entities,
            Some(&schema),  // ← Schema para validación
        )?;
        
        // ...
    }
}
```

---

## 📊 Resumen de Validación

| Requisito | Estado | Notas |
|-----------|--------|-------|
| Registrar tipos directamente (no strings) | ✅ CUMPLIDO | Via `register_entity<T>()` y `register_action_type<A>()` |
| Principal/Resource para Cedar | ✅ CUMPLIDO | Ambos heredan de `HodeiEntityType` |
| Extracción automática de fragments | ✅ CUMPLIDO | Funciones ya implementadas en `EngineBuilder` |
| Persistencia interna | ✅ CUMPLIDO | Dentro de `build_and_persist_schema` |
| Recuperación para validate/evaluate | ✅ CUMPLIDO | Via `SchemaLoaderPort` en ambos use cases |

---

## 🎯 Arquitectura Final Validada

### Componentes Principales

1. **EngineBuilder (existente)**
   - Registro síncrono en memoria
   - Métodos genéricos con tipos
   - Extracción automática de fragments
   - ✅ No necesita Port

2. **Feature: build_and_persist_schema (nuevo)**
   - Construye schema final
   - Persiste automáticamente si cambió
   - Port: `SchemaStoragePort` (async)

3. **Feature: load_schema (nuevo)**
   - Carga schema persistido
   - Port: `SchemaLoaderPort` (async)

4. **Modificaciones en validate_policy**
   - Añadir dependencia: `SchemaLoaderPort`
   - Validar con schema cargado

5. **Modificaciones en evaluate_policies**
   - Añadir dependencia: `SchemaLoaderPort`
   - Evaluar con schema cargado

6. **Feature: register_iam_types (nuevo en hodei-iam)**
   - Recibe `&mut EngineBuilder`
   - Registra todos los tipos IAM
   - ✅ Sin ports (síncrono)

---

## 🔄 Flujo Completo Validado

```
[main.rs - Arranque]
        ↓
[Crear EngineBuilder]
let mut builder = EngineBuilder::new();
        ↓
[hodei-iam registra tipos]
register_iam_types.execute(&mut builder, ...)?;
    → builder.register_entity::<User>()?;
    → builder.register_entity::<Group>()?;
    → builder.register_action_type::<CreateUserAction>()?;
        ↓
[hodei-orgs registra tipos]
register_org_types.execute(&mut builder, ...)?;
        ↓
[hodei-artifacts registra tipos]
register_artifact_types.execute(&mut builder, ...)?;
        ↓
[Construir y persistir]
build_and_persist.execute(builder, ...).await?;
    1. schema = builder.build_schema()?       [consume builder]
    2. hash = calculate_hash(&schema)
    3. existing = storage.load_latest().await?
    4. if existing.hash != hash {
           storage.save(schema, hash).await?  [INTERNO]
       }
    5. return SchemaView
        ↓
[Inicializar use cases]
let validate_uc = ValidatePolicyUseCase::new(schema_loader);
let evaluate_uc = EvaluatePoliciesUseCase::new(schema_loader);
        ↓
[Durante ejecución]
validate_uc.execute(cmd).await?
    → schema = schema_loader.load_schema().await?
    → validar con schema
        ↓
evaluate_uc.execute(cmd).await?
    → schema = schema_loader.load_schema().await?
    → evaluar con schema
```

---

## ✅ Confirmaciones Finales

### 1. Uso de Tipos Directos
✅ **CONFIRMADO**: El `EngineBuilder` usa tipos genéricos, no strings
```rust
// ✅ CORRECTO
builder.register_entity::<User>()?;
builder.register_action_type::<CreateUserAction>()?;

// ❌ NO HACEMOS ESTO
builder.register_entity("User", "schema string")?;
```

### 2. Principal y Resource
✅ **CONFIRMADO**: Ambos traits se usan para Cedar
```rust
pub trait Principal: HodeiEntity + HodeiEntityType {}
pub trait Resource: HodeiEntity + HodeiEntityType {}

// User implementa Principal
impl Principal for User {}

// Se registra usando HodeiEntityType (que Principal hereda)
builder.register_entity::<User>()?;
```

### 3. Extracción Automática
✅ **CONFIRMADO**: El `EngineBuilder` genera fragments automáticamente
```rust
// Dentro de EngineBuilder
fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment> {
    let service = T::service_name();
    let resource_type = T::resource_type_name();
    let attributes = T::attributes_schema();
    
    // Genera Cedar DSL automáticamente
    // ...
}
```

### 4. Persistencia Interna
✅ **CONFIRMADO**: Se persiste dentro del use case, no como feature separada
```rust
pub struct BuildAndPersistSchemaUseCase<SS> {
    schema_storage: Arc<SS>,  // Port para persistir
}

// La persistencia es INTERNA al execute()
pub async fn execute(&self, builder: EngineBuilder) -> Result<SchemaView> {
    let schema = builder.build_schema()?;
    // ... compara y persiste internamente ...
}
```

### 5. Recuperación para Validación/Evaluación
✅ **CONFIRMADO**: Ambos use cases cargan el schema
```rust
// validate_policy
pub struct ValidatePolicyUseCase<SL> {
    schema_loader: Arc<SL>,  // ← Para cargar schema
}

// evaluate_policies
pub struct EvaluatePoliciesUseCase<SL> {
    schema_loader: Arc<SL>,  // ← Para cargar schema
}
```

---

## 🎯 Conclusión

**TODOS los requisitos del usuario están CUMPLIDOS** ✅

La arquitectura corregida:

1. ✅ Usa tipos directos (`T: HodeiEntityType`, `A: ActionTrait`)
2. ✅ Respeta `Principal` y `Resource` como tipos Cedar
3. ✅ Extrae schema fragments automáticamente
4. ✅ Persistencia es interna a `build_and_persist_schema`
5. ✅ Schema se recupera para validación y evaluación

**Diferencia clave con arquitectura anterior:**
- ❌ ANTES: Ports para registro (innecesario)
- ✅ AHORA: EngineBuilder directo (síncrono, type-safe)

**No necesitamos:**
- ❌ `SchemaBuilderPort` para registro
- ❌ Features separadas `register_entity_type` / `register_action_type`
- ❌ Feature separada `persist_schema`

**Sí necesitamos:**
- ✅ `SchemaStoragePort` (async, para persistir/cargar)
- ✅ `SchemaLoaderPort` (async, para validar/evaluar)
- ✅ Feature `build_and_persist_schema` (construir + persistir interno)
- ✅ Feature `load_schema` (cargar para uso)
- ✅ Feature `register_iam_types` por cada BC (síncrono, usa builder directamente)

---

## 📝 Próximos Pasos Validados

1. ✅ Crear `EvaluatePoliciesPort` (Fase 1)
2. ✅ Actualizar hodei-iam para usar port (Fase 1)
3. ✅ Implementar `build_and_persist_schema` feature
4. ✅ Implementar `load_schema` feature
5. ✅ Modificar `validate_policy` con `SchemaLoaderPort`
6. ✅ Modificar `evaluate_policies` con `SchemaLoaderPort`
7. ✅ Implementar `register_iam_types` feature
8. ✅ Crear módulo `internal/actions/` con actions IAM
9. ✅ Integrar en main.rs

**Tiempo estimado total:** ~12.5 horas

---

**Estado:** ✅ **ARQUITECTURA VALIDADA Y LISTA PARA IMPLEMENTACIÓN**