Claro. Aquí tienes la versión definitiva y completa del plan de acción. Este documento unifica todas nuestras conversaciones, integrando la arquitectura de delegación, el aislamiento total de `cedar-policy`, la limpieza de código obsoleto y la introducción de Value Objects para un diseño de dominio robusto.

Este es el plan maestro para la implementación.

---

### **Plan de Acción Definitivo: Implementación del Monolito Modular Descomponible**

#### **Visión Arquitectónica:**
Construir un monolito modular donde cada `crate` es un _bounded context_ autónomo, dueño de sus propios datos y lógicas. La comunicación para la autorización se basa en un modelo de **orquestación y delegación síncrona** a través de interfaces abstractas. El motor de políticas (`cedar-policy`) es un **detalle de implementación completamente encapsulado dentro del `crate` `policies`**, invisible para el resto del sistema. El `shared kernel` utiliza **Value Objects** para un lenguaje de dominio tipado, robusto y auto-validado, asegurando la máxima flexibilidad, desacoplamiento y la preparación para una futura extracción a microservicios.

---

### **Épica 1: Crear un Kernel de Dominio Tipado y Agnóstico (`shared`)**

*   **Objetivo:** Establecer un lenguaje de dominio robusto, explícito y validado por el compilador, completamente aislado de dependencias externas.

*   **HU-1.1: Definir los Value Objects del Dominio**
    *   **Como** arquitecto, **quiero** crear `structs` `newtype` como `ServiceName`, `ResourceTypeName`, `AttributeName` en el `crate` `shared`.
    *   **Para que** el lenguaje del dominio sea explícito y auto-validado, previniendo errores de formato en tiempo de compilación.
    *   **Algoritmo:**
        1.  Crear `crates/shared/src/domain/value_objects.rs`.
        2.  Definir `pub struct ServiceName(String);` y los demás, con constructores privados.
        3.  Implementar un método `pub fn new(value: impl Into<String>) -> Result<Self, ValidationError>` para cada uno, conteniendo la lógica de validación de formato (p. ej., `ServiceName` debe ser `lowercase-kebab-case`).
        4.  Implementar `Deref<Target=String>` y `AsRef<str>` para un uso ergonómico.
    *   **Criterios de Aceptación:**
        *   Los tests unitarios para cada Value Object verifican sus reglas de validación.

*   **HU-1.2: Definir Primitivas de Atributos Agnósticas**
    *   **Como** arquitecto, **quiero** definir un `enum` `AttributeValue` en `shared` que represente los tipos de datos de atributos sin depender de Cedar.
    *   **Para que** las entidades de dominio puedan describir sus atributos de forma independiente.
    *   **Algoritmo:**
        1.  En `crates/shared/src/domain/attributes.rs`, definir `pub enum AttributeValue { Bool(bool), Long(i64), String(String), Set(Vec<AttributeValue>), Record(HashMap<String, AttributeValue>) }`.
    *   **Criterios de Aceptación:**
        *   El `enum` `AttributeValue` existe en `shared` y no tiene dependencias de `cedar-policy`.

*   **HU-1.3: Redefinir `HodeiEntityType` y `HodeiEntity` para ser Agnósticos y Tipados**
    *   **Como** arquitecto, **quiero** que los `traits` de entidad en `shared` utilicen exclusivamente Value Objects y tipos agnósticos.
    *   **Para que** la implementación de entidades por parte de los dominios sea segura, semántica y no dependa de Cedar.
    *   **Algoritmo:**
        1.  En `crates/shared/src/domain/ports.rs`, refactorizar los `traits`:
            ```rust
            pub trait HodeiEntityType {
                fn service_name() -> ServiceName;
                fn resource_type_name() -> ResourceTypeName;
                fn entity_type_name() -> String; // Generado, e.g., "Iam::User"
                fn attributes_schema() -> Vec<(AttributeName, AttributeType)>;
            }

            pub trait HodeiEntity {
                fn get_hrn(&self) -> &Hrn;
                fn get_attributes(&self) -> HashMap<String, AttributeValue>;
                fn get_parent_hrns(&self) -> Vec<Hrn>;
            }
            ```
    *   **Criterios de Aceptación:**
        *   Los `traits` están en `shared` y no contienen ningún tipo del `crate` `cedar-policy`.

*   **HU-1.4: Actualizar las Entidades de Dominio para Implementar los `traits` Agnósticos y Tipados**
    *   **Como** desarrollador, **quiero** actualizar las implementaciones de `HodeiEntityType` y `HodeiEntity` para `User`, `Group`, `Account`, etc.
    *   **Para que** se ajusten al nuevo contrato y eliminen su dependencia de `cedar-policy`.
    *   **Algoritmo:**
        1.  En `crates/hodei-iam/src/shared/domain/entities.rs`, actualizar `impl HodeiEntityType for User` para que sus métodos devuelvan los nuevos Value Objects.
        2.  Actualizar `impl HodeiEntity for User` para que `get_attributes` devuelva `HashMap<String, AttributeValue>`.
        3.  Eliminar `use cedar_policy::...` de todos los ficheros de entidades de dominio.
        4.  Repetir para todas las demás entidades en todos los `crates` de dominio.
    *   **Criterios de Aceptación:**
        *   Los `crates` `hodei-iam` y `hodei-organizations` ya no tienen a `cedar-policy` como dependencia.

*   **HU-1.5: Definir los Puertos de Evaluación Delegada en `shared`**
    *   **Como** arquitecto, **quiero** definir los `traits` `ScpEvaluator` y `IamPolicyEvaluator` en `shared`.
    *   **Para que** `hodei-authorizer` pueda orquestar la evaluación de forma agnóstica.
    *   **Algoritmo:**
        1.  En `crates/shared/src/ports/authorization.rs`, definir los DTOs `EvaluationRequest` y `EvaluationDecision`.
        2.  Definir los `traits` `ScpEvaluator` y `IamPolicyEvaluator`.

*   **HU-1.6: Sellar los Límites de los Bounded Contexts**
    *   **Como** arquitecto, **quiero** hacer privados los módulos internos (`shared`) de `hodei-iam` y `hodei-organizations`.
    *   **Para que** la encapsulación sea forzada a nivel de compilador.
    *   **Algoritmo:** En los `lib.rs` de `hodei-iam` y `hodei-organizations`, cambiar `pub mod shared;` a `mod shared;`.

---

### **Épica 2: Convertir `policies` en un Traductor y Evaluador Aislado**

*   **Objetivo:** Encapsular toda la lógica y dependencias de `cedar-policy` exclusivamente dentro de este `crate`.

*   **HU-2.1: Implementar el Traductor de Tipos Agnósticos a Tipos Cedar**
    *   **Como** desarrollador de `policies`, **quiero** una capa de traducción que convierta las estructuras agnósticas de `shared` a sus equivalentes en Cedar.
    *   **Para que** el motor de evaluación pueda operar, manteniendo el resto del sistema desacoplado.
    *   **Algoritmo:**
        1.  Crear `crates/policies/src/translator.rs`.
        2.  Implementar `fn translate_attribute_value(...)` y `fn translate_to_cedar_entity(...)`.
    *   **Criterios de Aceptación:**
        *   El `crate` `policies` es el único que contiene esta lógica de traducción.

*   **HU-2.2: Redefinir el `AuthorizationEngine` para Usar el Traductor**
    *   **Como** desarrollador, **quiero** que la interfaz pública del `AuthorizationEngine` acepte tipos agnósticos.
    *   **Para que** actúe como una fachada simple para los `crates` de dominio.
    *   **Algoritmo:**
        1.  Definir `struct EngineRequest` en `policies` que use `&dyn HodeiEntity`, etc.
        2.  Refactorizar el método `is_authorized` del `AuthorizationEngine` para que acepte `EngineRequest` y use el `translator` internamente.
    *   **Criterios de Aceptación:**
        *   La firma pública del `AuthorizationEngine` es 100% agnóstica a Cedar.

*   **HU-2.3: Eliminar las `features` de Gestión y Persistencia de `policies`**
    *   **Como** arquitecto, **quiero** eliminar todo el código de CRUD y persistencia del `crate` `policies`.
    *   **Para que** su rol como biblioteca de lógica pura sea explícito.
    *   **Algoritmo:**
        1.  Eliminar todos los directorios de `features` de `policies` (`create_policy`, `delete_policy`, etc.).
        2.  Eliminar `PolicyStore`, `PolicyStorage` y las implementaciones de infraestructura relacionadas.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** `policies` ya no tiene `features` de CRUD ni capa de `infrastructure`. Su API pública es esencialmente el `AuthorizationEngine` y su `EngineBuilder`.

---

### **Épica 3: Transformar los Dominios en Evaluadores y Gestores Autónomos**

*   **Objetivo:** Hacer que cada `crate` sea completamente responsable de la gestión y evaluación de sus propias políticas.

*   **HU-3.1: `hodei-organizations` Gestiona y Evalúa sus Propios SCPs**
    *   **Como** desarrollador de Organizaciones, **quiero** que mi `crate` exponga casos de uso para el ciclo de vida completo de los SCPs y para su evaluación.
    *   **Para que** mi dominio sea autónomo.
    *   **Algoritmo:**
        1.  Implementar casos de uso de CRUD para los SCPs que operen sobre la DB de `organizations`.
        2.  Implementar `EvaluateScpsUseCase` (que implementa `ScpEvaluator`), que recolecta sus entidades y políticas, construye el `EngineRequest` agnóstico y llama a su instancia del `AuthorizationEngine`.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** El código obsoleto (`OrganizationBoundaryProvider`, `hierarchy_service.rs`) ha sido eliminado.

*   **HU-3.2: `hodei-iam` Gestiona y Evalúa sus Propias Políticas de Identidad**
    *   **Como** desarrollador de IAM, **quiero** que mi `crate` sea el único responsable de gestionar y evaluar las políticas de identidad.
    *   **Para que** el dominio de IAM sea autónomo.
    *   **Algoritmo y Criterios:** Análogos a `HU-3.1` pero para el dominio de IAM.
        *   **[Limpieza]** El código obsoleto (`IamPolicyProvider`) ha sido eliminado.

---

### **Épica 4: Simplificar `hodei-authorizer` a un Orquestador Puro**

*   **Objetivo:** Convertir el `authorizer` en un componente sin estado, simple y robusto.

*   **HU-4.1: Refactorizar `EvaluatePermissionsUseCase` para Orquestar y Delegar**
    *   **Como** desarrollador de Authorizer, **quiero** que mi caso de uso solo llame a los `traits` de evaluación y combine los resultados según la lógica de AWS.
    *   **Para que** el `authorizer` sea fácil de mantener y probar.
    *   **Algoritmo:**
        1.  Refactorizar `EvaluatePermissionsUseCase` para que dependa de `Arc<dyn ScpEvaluator>` y `Arc<dyn IamPolicyEvaluator>`.
        2.  Implementar el método `execute` siguiendo el flujo de AWS (SCP primero, `Deny` anula todo, luego IAM).
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** El fichero `authorizer.rs` y las dependencias directas a `hodei-iam` y `hodei-organizations` han sido eliminados.

---

### **Épica 5: Componer y Exponer la Aplicación Monolítica (en `hodei-artifacts-api`)**

*   **Objetivo:** "Cablear" los componentes desacoplados en el `crate` binario y exponer una API coherente.

*   **HU-5.1: Simplificar `AppState` para Exponer solo Casos de Uso de API**
    *   **Como** desarrollador, **quiero** refactorizar `AppState` para que solo contenga los puntos de entrada que los `handlers` de la API necesitan.
    *   **Para que** el estado compartido sea mínimo y refleje la arquitectura.
    *   **Algoritmo:**
        1.  Modificar `src/app_state.rs` para que contenga únicamente los `Arc<...UseCase>` que los `handlers` de API llaman directamente (p. ej., `authorizer_uc`, `create_user_uc`, `create_scp_uc`).
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** Se eliminan del `AppState` las referencias directas a repositorios y a casos de uso internos.

*   **HU-5.2: Implementar el `Composition Root` en `build_app_state`**
    *   **Como** desarrollador, **quiero** que `build_app_state` ensamble la cadena de dependencias completa.
    *   **Para que** el "cableado" de la aplicación sea explícito y centralizado.
    *   **Algoritmo:**
        1.  En `build_app_state` (`src/lib.rs`):
            a. Construir el `Schema` global de Cedar usando el `EngineBuilder` y registrando las entidades de todos los dominios.
            b. Instanciar un `AuthorizationEngine` para `iam` y otro para `organizations`, ambos con el mismo `Schema`.
            c. Usar las funciones `di` para crear los evaluadores `iam_evaluator` y `scp_evaluator`, inyectándoles sus dependencias.
            d. Crear el `authorizer_uc` inyectando los evaluadores.
            e. Instanciar y almacenar en `AppState` el `authorizer_uc` y todos los demás casos de uso de gestión/API.

*   **HU-5.3: Unificar Endpoints de API por Dominio y Refactorizar Handlers**
    *   **Como** desarrollador de la API, **quiero** que la estructura de la API refleje los dominios autónomos.
    *   **Para que** la API sea coherente con la arquitectura.
    *   **Algoritmo:**
        1.  Refactorizar `src/api/` para que esté organizado por dominios (`iam.rs`, `organizations.rs`, `authorization.rs`).
        2.  Asegurarse de que cada `handler` solo contenga lógica de mapeo HTTP-DTO y llame al `UseCase` correspondiente del `AppState`.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** El `handler` `authorize` ya no contiene lógica de autorización _mock_.
        *   **[Limpieza]** Los `handlers` de listado ya no llaman a repositorios directamente.
        *   **[Limpieza]** El fichero `policy_handlers.rs` ha sido eliminado y sus responsabilidades distribuidas a los `handlers` de `iam.rs` y `organizations.rs`.

*   **HU-5.4: Implementar Fiabilidad de Eventos con Transactional Outbox (Opcional pero Recomendado)**
    *   **Como** desarrollador, **quiero** integrar el patrón Outbox para garantizar la fiabilidad de los eventos de auditoría y notificaciones externas.
    *   **Para que** el sistema sea robusto y no pierda información crítica.
    *   **Algoritmo:**
        1.  Implementar el `OutboxEventRepository`.
        2.  Extender la `UnitOfWork` para usar el outbox.
        3.  Refactorizar los casos de uso de escritura para que usen `uow.add_event()`.
        4.  Iniciar el `RelayWorker` en `main.rs`.
