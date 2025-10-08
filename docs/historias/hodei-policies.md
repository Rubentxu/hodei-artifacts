### **Documento de Diseño e Historias de Usuario: Crate `hodei-policies`**

**Visión General del Crate:**

El `crate` `hodei-policies` es el único _bounded context_ con conocimiento del motor de autorización Cedar. Actúa como un servicio puro y sin estado para el resto del sistema. Sus responsabilidades son:

1.  **Validar la sintaxis** de documentos de políticas proporcionados como `String`.
2.  **Evaluar un conjunto de políticas** (`HodeiPolicySet`) contra una petición de autorización (Principal, Acción, Recurso, Contexto) y devolver una decisión (`Allow`/`Deny`).

**Dependencias Principales:** `cedar-policy`, `kernel` (para `HodeiPolicy`, `HodeiEntity`, etc.), `async-trait`, `thiserror`, `serde`.

---

### **Épica 1: Funcionalidad de Validación de Políticas**

#### **HU-POL-001: Validar la Sintaxis de un Documento de Política**
*   **Como:** Un servicio externo (p. ej., el `UseCase` de `create_policy` en `hodei-iam`).
*   **Quiero:** Enviar un `String` que contiene un documento de política.
*   **Para:** Recibir una respuesta clara que indique si la sintaxis del documento es válida según las reglas de Cedar, junto con una lista de errores si es inválida.

##### **Estructura de la `feature` VSA: `validate_policy`**
```
crates/hodei-policies/src/features/validate_policy/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
└── use_case_test.rs
```

##### **Detalles de Implementación:**

1.  **`dto.rs`:**
    ```rust
    // Comando de entrada
    pub struct ValidatePolicyCommand {
        pub content: String,
    }

    // DTO de respuesta
    pub struct ValidationResult {
        pub is_valid: bool,
        pub errors: Vec<String>,
    }
    ```

2.  **`error.rs`:**
    ```rust
    #[derive(Debug, thiserror::Error)]
    pub enum ValidatePolicyError {
        #[error("An unexpected internal error occurred: {0}")]
        InternalError(String),
    }
    ```

3.  **`use_case.rs` (Algoritmo):**
    *   Este `UseCase` no tiene dependencias externas, por lo que no necesita un `ports.rs`.
    *   El método `execute` recibe el `ValidatePolicyCommand`.
    *   **Algoritmo:**
        1.  Validar el input: Si `command.content` está vacío o solo contiene espacios en blanco, devolver `ValidationResult { is_valid: false, errors: vec!["Policy content cannot be empty".to_string()] }`.
        2.  Utilizar Cedar para parsear el `String`: `cedar_policy::PolicySet::from_str(&command.content)`.
        3.  Si el resultado es `Ok(_)`, la sintaxis es válida. Devolver `ValidationResult { is_valid: true, errors: vec![] }`.
        4.  Si el resultado es `Err(e)`, la sintaxis es inválida. Formatear los errores de Cedar en un `Vec<String>`. Devolver `ValidationResult { is_valid: false, errors }`.

##### **Tests (`use_case_test.rs`):**

```rust
use super::use_case::ValidatePolicyUseCase;
use super::dto::ValidatePolicyCommand;

#[tokio::test]
async fn test_valid_policy_returns_is_valid_true() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand { content: "permit(principal, action, resource);".to_string() };
    let result = use_case.execute(command).await.unwrap();
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_invalid_policy_returns_is_valid_false_with_errors() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand { content: "permit(principal, action);".to_string() }; // Sintaxis incorrecta
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("wrong number of arguments"));
}

#[tokio::test]
async fn test_empty_policy_is_invalid() {
    let use_case = ValidatePolicyUseCase::new();
    let command = ValidatePolicyCommand { content: "   ".to_string() };
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert_eq!(result.errors[0], "Policy content cannot be empty");
}
```

---

### **Épica 2: Funcionalidad de Evaluación de Políticas**

Esta es la funcionalidad principal, que reutilizará la lógica del antiguo `AuthorizationEngine`.

#### **HU-POL-002: Evaluar una Petición de Autorización**
*   **Como:** Un servicio orquestador (p. ej., `hodei-authorizer`).
*   **Quiero:** Enviar un conjunto de políticas agnósticas (`HodeiPolicySet`), un conjunto de entidades agnósticas (`Vec<&dyn HodeiEntity>`), y una petición de autorización.
*   **Para:** Recibir una decisión final (`Allow`/`Deny`) basada en la evaluación de Cedar, sin exponer ningún detalle de Cedar.

##### **Estructura de la `feature` VSA: `evaluate_policies`**
```
crates/hodei-policies/src/features/evaluate_policies/
├── mod.rs
├── use_case.rs
├── dto.rs
├── error.rs
├── internal/
│   ├── translator.rs
│   └── schema_builder.rs
└── use_case_test.rs
```

##### **Detalles de Implementación:**

1.  **`dto.rs` (Contrato Público):**
    ```rust
    use kernel::domain::policy::HodeiPolicySet;
    use kernel::Hrn;
    use std::collections::HashMap;

    // Comando de entrada
    pub struct EvaluatePoliciesCommand<'a> {
        pub request: AuthorizationRequest<'a>,
        pub policies: &'a HodeiPolicySet,
        pub entities: &'a [&'a dyn kernel::HodeiEntity],
    }

    pub struct AuthorizationRequest<'a> {
        pub principal_hrn: &'a Hrn,
        pub action: &'a str,
        pub resource_hrn: &'a Hrn,
        pub context: Option<HashMap<String, serde_json::Value>>, // Contexto simple para la evaluación
    }
    
    // DTO de respuesta
    #[derive(Debug, PartialEq, Eq)]
    pub enum Decision { Allow, Deny }

    pub struct EvaluationDecision {
        pub decision: Decision,
        pub determining_policies: Vec<String>, // IDs de las políticas que llevaron a la decisión
        pub reasons: Vec<String>, // Explicaciones de Cedar
    }
    ```

2.  **`internal/translator.rs` (Lógica de Traducción, `pub(crate)`):**
    *   Aquí es donde se migra la lógica del antiguo `translator` y se adapta para usar `Hrn` y `HodeiEntity`.
    *   **`pub(crate) fn to_cedar_euid(hrn: &Hrn) -> Result<EntityUid, ...>`:** Traduce un `Hrn` a un `EntityUid` de Cedar. Algoritmo: `format!("{}::\"{}\"", HodeiEntityType::entity_type_name(), hrn.resource_id())`.
    *   **`pub(crate) fn to_cedar_entity(entity: &dyn HodeiEntity) -> Result<Entity, ...>`:** Traduce una entidad agnóstica a una de Cedar.
    *   **`pub(crate) fn to_cedar_policy_set(set: &HodeiPolicySet) -> Result<PolicySet, ...>`:** Traduce el `HodeiPolicySet` a un `PolicySet` de Cedar.

3.  **`internal/schema_builder.rs` (Lógica de Esquema, `pub(crate)`):**
    *   Migra la lógica del `EngineBuilder` antiguo.
    *   **`pub(crate) fn build_schema_from_entities(entities: &[&dyn HodeiEntity]) -> Result<Schema, ...>`:**
        *   **Algoritmo:**
            1.  Itera sobre `entities`.
            2.  Para cada `entity`, obtiene su `HodeiEntityType`.
            3.  Usa `HodeiEntityType::entity_type_name()` y `attributes_schema()` para generar un fragmento de esquema Cedar en formato `String`.
            4.  Mantiene un `HashSet` de nombres de tipo para no generar duplicados.
            5.  Combina todos los `String`s de fragmentos en un único `String` de esquema.
            6.  Parsea este `String` en un `cedar_policy::Schema`.

4.  **`use_case.rs` (Orquestador Principal):**
    *   `EvaluatePoliciesUseCase` no tiene dependencias externas, por lo que no necesita `ports.rs`.
    *   **Algoritmo de `execute`:**
        1.  **Construir Esquema:** Llama a `internal::schema_builder::build_schema_from_entities(command.entities)`.
        2.  **Traducir Políticas:** Llama a `internal::translator::to_cedar_policy_set(command.policies)`.
        3.  **Traducir Entidades:** Itera sobre `command.entities` y los traduce a un `Vec<cedar_policy::Entity>`, y luego a `cedar_policy::Entities`.
        4.  **Construir Petición de Cedar:**
            *   Traduce `principal_hrn` y `resource_hrn` a `EntityUid`.
            *   Crea el `EntityUid` de la acción (p. ej., `Action::"ReadDocument"`).
            *   Crea el `Context` de Cedar a partir del `HashMap` del DTO.
            *   Construye el `cedar_policy::Request`.
        5.  **Evaluar:**
            *   Crea una instancia de `cedar_policy::Authorizer`.
            *   Llama a `authorizer.is_authorized(&request, &policies, &entities)`.
        6.  **Mapear Respuesta:**
            *   Inspecciona la `Response` de Cedar.
            *   Si `response.decision() == cedar_policy::Decision::Allow`, devuelve `EvaluationDecision { decision: Decision::Allow, ... }`.
            *   Si no, devuelve `Decision::Deny`.
            *   Extrae los `reasons` y los `determining_policies` de la respuesta de Cedar y los incluye en el DTO de respuesta.

##### **Tests de Integración (`/tests/` del crate `hodei-policies`):**

Estos tests son cruciales porque validan el `UseCase` de caja negra.
```rust
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
// ... otros imports para entidades mock ...

// Se necesitarán entidades mock que implementen HodeiEntity, HodeiEntityType, etc.
// (ej. MockUser, MockDocument)

#[tokio::test]
async fn test_evaluation_allow_scenario() {
    let use_case = EvaluatePoliciesUseCase::new();

    // 1. Definir políticas agnósticas
    let policy = HodeiPolicy::new(
        PolicyId::new("p1"),
        "permit(principal == MockUser::\"alice\", action == Action::\"view\", resource == MockDocument::\"doc1\");".to_string(),
        None,
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    // 2. Definir entidades agnósticas
    let alice = MockUser::new("alice");
    let doc1 = MockDocument::new("doc1");
    let entities: Vec<&dyn HodeiEntity> = vec![&alice, &doc1];

    // 3. Crear la petición
    let request = AuthorizationRequest {
        principal_hrn: alice.hrn(),
        action: "view",
        resource_hrn: doc1.hrn(),
        context: None,
    };
    
    let command = EvaluatePoliciesCommand {
        request,
        policies: &policy_set,
        entities: &entities,
    };

    // Act
    let result = use_case.execute(command).await.unwrap();
    
    // Assert
    assert_eq!(result.decision, Decision::Allow);
    assert!(result.determining_policies.contains(&"p1".to_string()));
}

#[tokio::test]
async fn test_evaluation_deny_scenario() {
    // Similar al anterior, pero con una política `forbid` o una petición que no coincide.
    // ...
    // Assert
    // assert_eq!(result.decision, Decision::Deny);
}
```

---

### **Épica 3: API Pública y Deprecación**

#### **HU-POL-003: Definir la API Pública del Crate**
*   **Como:** Arquitecto.
*   **Quiero:** Exponer públicamente solo los casos de uso y sus DTOs asociados a través de `lib.rs` y `api.rs`.
*   **Para:** Proporcionar una superficie de API limpia, estable y desacoplada.
*   **Criterios de Aceptación:**
    1.  El `lib.rs` de `hodei-policies` debe tener la estructura `pub mod api; pub use api::*;`.
    2.  El `api.rs` debe exportar los módulos de `features` completos.
    3.  Los módulos internos como `translator` y `schema_builder` NO deben ser públicos.

#### **HU-DEP-002 (Recordatorio): Eliminar el `crate` `policies` Obsoleto**
*   **Como:** Desarrollador.
*   **Quiero:** Eliminar por completo el antiguo `crate` `policies` del `workspace`.
*   **Para:** Finalizar la migración y evitar confusiones.
*   **Criterios de Aceptación:**
    1.  Verificar que ningún `crate` depende ya del `crate` `policies`.
    2.  Eliminar `policies` del `Cargo.toml` del `workspace` y de la estructura de directorios.

Este plan de acción detallado para `hodei-policies` establece una base sólida para el resto de la re-arquitectura. Crea un servicio de políticas puro, reutiliza la lógica existente de manera segura y proporciona los contratos agnósticos que los demás `crates` necesitarán para delegar la lógica de autorización.