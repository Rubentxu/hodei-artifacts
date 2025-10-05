### **Plan de Acción Definitivo: Implementación del Monolito Modular Descomponible**

#### **Visión Arquitectónica:**
Construir un monolito modular donde cada `crate` es un _bounded context_ autónomo, dueño de sus propios datos y políticas. La comunicación para la autorización se basa en un modelo de **orquestación y delegación síncrona**, garantizando la consistencia fuerte y la preparación para una futura extracción a microservicios. Los eventos de dominio se emiten de forma asíncrona y fiable para auditoría y sistemas externos.

---

### **Épica 1: Establecer los Contratos y Límites Arquitectónicos Fundamentales**

*   **Objetivo:** Crear una base arquitectónica limpia, eliminando el acoplamiento indebido y estableciendo los contratos de comunicación en el `shared` kernel.

*   **HU-1.1: Centralizar las Abstracciones de Dominio en el Kernel `shared`**
    *   **Como** arquitecto, **quiero** mover los `traits` y `structs` de dominio compartidos (`Hrn`, `HodeiEntity`, `HodeiEntityType`, `Principal`, `Resource`, `ActionTrait`) desde `policies` al `crate` `shared`.
    *   **Para que** todos los `crates` dependan de un kernel común y estable.
    *   **Algoritmo:**
        1.  Mover los ficheros `hrn.rs` y `ports.rs` de `policies/src/shared/domain` a `shared/src/domain`.
        2.  Añadir `shared` como dependencia en `crates/policies/Cargo.toml`.
        3.  Actualizar globalmente todas las declaraciones `use` para apuntar al `crate` `shared`.
    *   **Criterios de Aceptación:**
        *   El proyecto compila correctamente.
        *   **[Limpieza]** El directorio `crates/policies/src/shared/domain/` ya no contiene los ficheros movidos.

*   **HU-1.2: Sellar los Límites de los Bounded Contexts**
    *   **Como** arquitecto, **quiero** hacer privados los módulos internos (`shared`) de `hodei-iam` y `hodei-organizations`.
    *   **Para que** sea imposible acceder a sus detalles de implementación desde fuera, forzando el uso de la API pública de casos de uso.
    *   **Algoritmo:** En los `lib.rs` de `hodei-iam` y `hodei-organizations`, cambiar `pub mod shared;` a `mod shared;`.
    *   **Criterios de Aceptación:**
        *   Un intento de importar una entidad interna desde otro `crate` (`use hodei_iam::shared::domain::User;`) provoca un error de compilación de visibilidad.

*   **HU-1.3: Definir los Puertos de Evaluación Delegada en `shared`**
    *   **Como** arquitecto, **quiero** definir los `traits` `ScpEvaluator` y `IamPolicyEvaluator` en `shared`.
    *   **Para que** `hodei-authorizer` pueda orquestar la evaluación de forma agnóstica.
    *   **Algoritmo:**
        1.  Crear `crates/shared/src/ports/authorization.rs`.
        2.  Definir los DTOs `EvaluationRequest` y `EvaluationDecision` (`Allow`, `Deny`, `NotApplicable`).
        3.  Definir los `traits` `ScpEvaluator` y `IamPolicyEvaluator` con sus métodos de evaluación.

---

### **Épica 2: Simplificar el `crate` `policies` a una Biblioteca de Lógica Pura**

*   **Objetivo:** Refactorizar `policies` para que actúe puramente como una biblioteca de lógica de negocio compartida, eliminando responsabilidades de persistencia y gestión que no le corresponden.

*   **HU-2.1: Eliminar las `features` de Gestión de Políticas de `policies`**
    *   **Como** arquitecto, **quiero** eliminar todos los casos de uso de CRUD de políticas del `crate` `policies`.
    *   **Para que** la responsabilidad de gestionar el ciclo de vida de las políticas recaiga exclusivamente en los dominios autónomos (`iam` y `organizations`).
    *   **Algoritmo:**
        1.  Eliminar los directorios `create_policy`, `delete_policy`, `update_policy`, `get_policy`, y `list_policies` de `crates/policies/src/features/`.
        2.  Limpiar las referencias a estos módulos en `features/mod.rs` y `lib.rs`.
        3.  Eliminar los tests de integración asociados en `crates/policies/tests/`.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** El `crate` `policies` ya no compila ningún caso de uso de CRUD de políticas.

*   **HU-2.2: Eliminar la Capa de Persistencia de `policies`**
    *   **Como** arquitecto, **quiero** que el `AuthorizationEngine` sea completamente sin estado.
    *   **Para que** su rol como biblioteca de lógica pura sea explícito y no tenga dependencias de infraestructura.
    *   **Algoritmo:**
        1.  Refactorizar `AuthorizationEngine` para que solo contenga el `schema: Arc<Schema>` y los métodos de evaluación.
        2.  Eliminar `PolicyStore` y el `trait PolicyStorage`.
        3.  Eliminar las implementaciones de `PolicyStorage` (`SurrealMemStorage`, `SurrealEmbeddedStorage`) de `crates/policies/src/shared/infrastructure/`.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** `PolicyStore`, `PolicyStorage` y sus implementaciones han sido eliminados del `crate` `policies`.
        *   `AuthorizationEngine` es ahora un `struct` sin estado que solo realiza evaluaciones.

---

### **Épica 3: Transformar los Dominios en Evaluadores y Gestores Autónomos**

*   **Objetivo:** Hacer que cada `crate` (`iam`, `organizations`) sea completamente responsable de la gestión y evaluación de sus propias políticas.

*   **HU-3.1: `hodei-organizations` Gestiona y Evalúa sus Propios SCPs**
    *   **Como** desarrollador de Organizaciones, **quiero** que mi `crate` exponga casos de uso para el ciclo de vida completo de los SCPs y para su evaluación.
    *   **Para que** mi dominio sea autónomo y el único experto en SCPs.
    *   **Algoritmo:**
        1.  Asegurarse de que el `UnitOfWork` de `hodei-organizations` incluye un `ScpRepository` que opera sobre su propia base de datos.
        2.  Implementar los casos de uso de CRUD para los SCPs (p. ej., `CreateScpUseCase`, `AttachScpUseCase`).
        3.  Implementar el `EvaluateScpsUseCase` que implementa `ScpEvaluator`, usando su `ScpRepository` interno y una instancia del `AuthorizationEngine`.
    *   **Criterios de Aceptación:**
        *   Es posible crear, adjuntar y evaluar un SCP a través de la API de `hodei-organizations`.
        *   **[Limpieza]** El `trait OrganizationBoundaryProvider` y su implementación han sido eliminados.
        *   **[Limpieza]** El fichero `hierarchy_service.rs` ha sido eliminado y su lógica internalizada.

*   **HU-3.2: `hodei-iam` Gestiona y Evalúa sus Propias Políticas de Identidad**
    *   **Como** desarrollador de IAM, **quiero** que mi `crate` sea el único responsable de gestionar y evaluar las políticas de identidad.
    *   **Para que** el dominio de IAM sea autónomo.
    *   **Algoritmo y Criterios:** Análogos a `HU-3.1` pero para las políticas de identidad de IAM.
        *   **[Limpieza]** El `trait IamPolicyProvider` y su implementación (`SurrealIamPolicyProvider`) han sido eliminados.

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
        *   El `authorizer` no contiene lógica para buscar políticas, grupos o jerarquías.
        *   **[Limpieza]** El fichero `crates/hodei-authorizer/src/authorizer.rs` ha sido eliminado.
        *   **[Limpieza]** El `Cargo.toml` de `hodei-authorizer` ya no contiene dependencias directas a `hodei-iam` ni `hodei-organizations`.

---

### **Épica 5: Componer y Exponer la Aplicación Monolítica (en `hodei-artifacts-api`)**

*   **Objetivo:** "Cablear" los componentes desacoplados en el `crate` binario y exponerlos a través de una API coherente.

*   **HU-5.1: Simplificar `AppState` para Reflejar la Arquitectura de Casos de Uso**
    *   **Como** desarrollador, **quiero** refactorizar `AppState` para que solo contenga los puntos de entrada principales de la API.
    *   **Para que** el estado compartido sea mínimo y refleje la arquitectura de componentes.
    *   **Algoritmo:**
        1.  Modificar `src/app_state.rs` para que contenga únicamente los `Arc<...UseCase>` que los `handlers` de la API necesitan llamar directamente (p. ej., `authorizer_uc`, `create_user_uc`, `create_scp_uc`, etc.).
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** Se eliminan del `AppState` las referencias directas a repositorios y a los casos de uso de evaluación internos que ahora son dependencias del `authorizer`.

*   **HU-5.2: Implementar el `Composition Root` en `build_app_state`**
    *   **Como** desarrollador, **quiero** que `build_app_state` ensamble la cadena de dependencias de autorización.
    *   **Para que** el "cableado" de la aplicación sea explícito y centralizado.
    *   **Algoritmo:**
        1.  En `build_app_state`:
            a. Instanciar repositorios y UoW factories para `iam` y `organizations`.
            b. Construir el `Schema` global usando el `EngineBuilder` de `policies`.
            c. Crear una instancia del `AuthorizationEngine` para cada dominio (`iam_engine`, `org_engine`).
            d. Usar las funciones `di` para crear los evaluadores `iam_evaluator` y `scp_evaluator`, inyectándoles sus dependencias.
            e. Crear el `authorizer_uc` inyectando los evaluadores.
            f. Instanciar y almacenar en `AppState` el `authorizer_uc` y todos los demás casos de uso de gestión.

*   **HU-5.3: Unificar los Endpoints de Gestión de Políticas bajo sus Dominios**
    *   **Como** desarrollador de la API, **quiero** que la creación de políticas se realice a través de endpoints de su dominio correspondiente (p. ej., `POST /iam/policies`, `POST /organizations/scps`).
    *   **Para que** la API refleje la estructura de dominios autónomos.
    *   **Algoritmo:**
        1.  Eliminar `src/api/policy_handlers.rs`.
        2.  En `src/api/iam.rs`, añadir `handlers` para `create_iam_policy`, `attach_iam_policy`, que llamen a los casos de uso correspondientes de `hodei-iam`.
        3.  Crear `src/api/organizations.rs` y hacer lo mismo para los SCPs.
    *   **Criterios de Aceptación:**
        *   **[Limpieza]** El fichero `policy_handlers.rs` ha sido eliminado. La API está organizada por dominios.

*   **HU-5.4: Implementar Fiabilidad de Eventos con Transactional Outbox (Opcional)**
    *   **Como** desarrollador, **quiero** integrar el patrón Outbox para garantizar la fiabilidad de los eventos de auditoría.
    *   **Para que** el sistema sea robusto y no pierda información crítica.
    *   **Algoritmo:**
        1.  Implementar la infraestructura del Outbox.
        2.  Refactorizar los casos de uso de escritura para que usen `uow.add_event()`.
        3.  Iniciar el `RelayWorker` en `main.rs` como una tarea en segundo plano.