### **Documento de Planificación: Implementación del Sistema de Autorización Multi-capa**

#### **Visión del Producto**

El objetivo es desarrollar un sistema de autorización robusto y multi-capa, 
inspirado en los servicios de AWS (IAM, Organizations, Access Analyzer, CloudTrail),
que se integre de forma nativa en nuestra arquitectura de crates y siga los principios de diseño VSA y Hexagonal. 
El sistema aplicará el principio de privilegio mínimo y la precedencia de la denegación explícita para garantizar la máxima seguridad.

---
Cada historia está diseñada para ser independiente (siempre que se cumplan sus dependencias), medible y alineada con tu arquitectura estricta. He añadido una sección de "Justificación" a cada una para explicar el "porqué" de esa segregación específica, junto con detalles de implementación más profundos.

---

### **Documento de Planificación Detallado: Implementación del Sistema de Autorización Multi-capa**

### **Epic 1: Refactorización y Alineamiento Arquitectónico (Deuda Técnica)**

**Objetivo:** Alinear el código existente con las directrices de arquitectura VSA estrictas para mejorar la consistencia, mantenibilidad y reducir la deuda técnica antes de construir nuevas funcionalidades.

---
#### **HU-1.1: Definir los Puertos Segregados para la feature `attach_scp`** ✅ **COMPLETADA**
*   **Como** desarrollador,
*   **quiero** definir los traits de puerto específicos que el caso de uso `AttachScpUseCase` necesita,
*   **para** cumplir con el Principio de Segregación de Interfaces (ISP) y desacoplar el caso de uso de las interfaces de repositorio completas.
*   **Justificación:** Esta HU establece el "contrato" que el caso de uso requiere de sus dependencias, sin acoplarlo a la totalidad de los métodos de un repositorio. Es el primer paso para una implementación VSA limpia.
*   **Detalles de Implementación:**
    *   **Fichero a Crear/Modificar:** `crates/hodei-organizations/src/features/attach_scp/ports.rs`.
    *   **Traits a Definir:**
        *   `trait ScpRepositoryPort`: Debe definir `async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>`.
        *   `trait AccountRepositoryPort`: Debe definir `async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>` y `async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError>`.
        *   `trait OuRepositoryPort`: Debe definir `async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>` y `async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError>`.
*   **Criterios de Aceptación:**
    1.  ✅ El fichero `ports.rs` existe y contiene los tres traits definidos.
    2.  ✅ Las firmas de los métodos son exactamente las necesarias para que `AttachScpUseCase` compile.
    3.  ✅ El código compila sin errores (`cargo check`).

---
#### **HU-1.2: Implementar los Adaptadores para los Puertos de `attach_scp`** ✅ **COMPLETADA**
*   **Como** desarrollador,
*   **quiero** implementar los adaptadores que conectan los puertos segregados de `attach_scp` con los repositorios de la capa de aplicación,
*   **para** completar el patrón hexagonal y permitir que el caso de uso interactúe con la infraestructura de forma indirecta.
*   **Justificación:** Esta HU es la "fontanería" que conecta la abstracción (el puerto) con la siguiente capa de abstracción (el repositorio de aplicación), permitiendo la inyección de dependencias.
*   **Detalles de Implementación:**
    *   **Fichero a Crear/Modificar:** `crates/hodei-organizations/src/features/attach_scp/adapter.rs`.
    *   **Structs a Definir:**
        *   `struct ScpRepositoryAdapter<R: ScpRepository>`: Implementará `ScpRepositoryPort` delegando las llamadas al `R` interno.
        *   `struct AccountRepositoryAdapter<R: AccountRepository>`: Implementará `AccountRepositoryPort` delegando las llamadas al `R` interno.
        *   `struct OuRepositoryAdapter<R: OuRepository>`: Implementará `OuRepositoryPort` delegando las llamadas al `R` interno.
*   **Criterios de Aceptación:**
    1.  ✅ El fichero `adapter.rs` existe y contiene las tres structs.
    2.  ✅ Cada `Adapter` implementa su `Port` correspondiente.
    3.  ✅ El código compila sin errores.
*   **Dependencias:** `HU-1.1`.

---
#### **HU-1.3: Refactorizar `AttachScpUseCase` y sus Tests para Usar Puertos Segregados** ✅ **COMPLETADA**
*   **Como** desarrollador,
*   **quiero** modificar el `AttachScpUseCase` y sus tests asociados para que dependan de los nuevos puertos segregados,
*   **para** finalizar el alineamiento VSA de la feature.
*   **Justificación:** Completa el refactor, asegurando que el núcleo de la lógica de negocio ahora depende de su propio contrato segregado, lo que facilita las pruebas y el mantenimiento.
*   **Detalles de Implementación:**
    *   **Ficheros a Modificar:**
        *   `features/attach_scp/use_case.rs`: Cambiar la firma de la struct y el constructor `new` para aceptar los `*Port` traits.
        *   `features/attach_scp/di.rs`: Actualizar la factoría para que construya los adaptadores y los inyecte en el caso de uso.
        *   `features/attach_scp/mocks.rs`: Modificar los mocks para que implementen los `*Port` traits en lugar de los traits de repositorio genéricos.
        *   `features/attach_scp/use_case_test.rs`: Asegurarse de que los tests sigan funcionando con los mocks actualizados.
*   **Criterios de Aceptación:**
    1.  ✅ `AttachScpUseCase` ya no depende directamente de los traits de `shared/application/ports`.
    2.  ✅ La inyección de dependencias funciona correctamente.
    3.  ✅ Todos los tests en `use_case_test.rs` pasan.
*   **Dependencias:** `HU-1.2`.

---
#### **HU-1.4: Definir el Contrato de `UnitOfWork` en el Shared Kernel** ✅ **COMPLETADA**
*   **Como** desarrollador,
*   **quiero** definir una abstracción `UnitOfWork` en el `shared kernel`,
*   **para** establecer un contrato estándar para gestionar transacciones que pueda ser implementado por diferentes proveedores de persistencia.
*   **Justificación:** Es la base para implementar operaciones atómicas. Definir primero el trait asegura que la lógica de negocio no se acople a una implementación de base de datos específica.
*   **Detalles de Implementación:**
    *   **Fichero a Crear:** `crates/shared/src/application/ports/unit_of_work.rs`.
    *   **Trait a Definir:**
        *   `trait UnitOfWork`: Debe ser `async`. Definirá métodos `async fn begin()`, `async fn commit()`, `async fn rollback()`.
        *   También expondrá "factorías" para los repositorios transaccionales: `fn accounts(&self) -> Arc<dyn AccountRepository>`. El repositorio devuelto debe estar ligado al contexto de la transacción.
*   **Criterios de Aceptación:**
    1.  ✅ El trait `UnitOfWork` está definido.
    2.  ✅ La decisión de diseño sobre cómo los repositorios participan en la transacción (obteniéndolos de la UoW) está documentada en el código.
    3.  ✅ El código compila sin errores (`cargo check`).

---
#### **HU-1.5: Implementar `SurrealUnitOfWork` y Repositorios Transaccionales** ✅ **COMPLETADA**
*   **Como** desarrollador,
*   **quiero** una implementación concreta de `UnitOfWork` para SurrealDB y adaptar los repositorios para que la utilicen,
*   **para** poder ejecutar operaciones de base de datos de manera transaccional.
*   **Justificación:** Proporciona la capacidad técnica real para realizar transacciones. Sin esto, la UoW es solo una abstracción.
*   **Detalles de Implementación:**
    *   **Fichero a Crear:** `crates/hodei-organizations/src/shared/infrastructure/surreal/unit_of_work.rs`.
    *   **Lógica/Algoritmo:**
        *   La struct `SurrealUnitOfWork` contendrá una conexión a la DB.
        *   `begin()` ejecutará la query `BEGIN TRANSACTION`.
        *   `commit()` ejecutará `COMMIT TRANSACTION`.
        *   `rollback()` ejecutará `CANCEL TRANSACTION`.
        *   Los `Surreal*Repository` existentes deben ser refactorizados. Sus métodos `save` y `update` ya no tomarán una conexión directa, sino una referencia al contexto transaccional gestionado por la `SurrealUnitOfWork`.
*   **Criterios de Aceptación:**
    1.  ✅ `SurrealUnitOfWork` implementa correctamente el trait `UnitOfWork`.
    2.  ✅ Los métodos de los repositorios de SurrealDB se han adaptado para operar dentro del contexto de una transacción.
    3.  ✅ El código compila sin errores (`cargo check`).
*   **Dependencias:** `HU-1.4`.

---
#### **HU-1.6: Aplicar Transaccionalidad al `MoveAccountUseCase`**
*   **Como** desarrollador,
*   **quiero** refactorizar `MoveAccountUseCase` para que utilice el `UnitOfWork`,
*   **para** asegurar que las tres operaciones de guardado (cuenta, OU origen, OU destino) se realicen de forma atómica.
*   **Justificación:** Corrige una vulnerabilidad crítica de consistencia de datos en una operación de negocio fundamental.
*   **Detalles de Implementación:**
    *   **Ficheros a Modificar:** `features/move_account/use_case.rs`, `di.rs`, y `ports.rs`.
    *   **Lógica/Algoritmo:**
        1.  Inyectar una factoría de `UnitOfWork` en `MoveAccountUseCase`.
        2.  En `execute`, llamar a `uow_factory.create()` para obtener una nueva UoW.
        3.  Llamar a `uow.begin()`.
        4.  Obtener los repositorios desde la `uow`.
        5.  Realizar todas las operaciones de lectura y escritura usando estos repositorios.
        6.  Envolver la lógica en un `match` o `if let Ok(...)`: si todo va bien, llamar a `uow.commit()`. Si cualquier `Result` es `Err`, llamar a `uow.rollback()` antes de propagar el error.
*   **Criterios de Aceptación:**
    1.  El `MoveAccountUseCase` ya no tiene fallos de atomicidad.
    2.  Los tests de integración se actualizan para simular un fallo a mitad de la operación y verificar que los datos se revierten a su estado original.
*   **Dependencias:** `HU-1.5`.

---
### **Epic 2: Implementar el Motor de Autorización Central (`hodei-authorizer`)**

**Objetivo:** Crear un nuevo *bounded context* (`hodei-authorizer`) que centralice todas las decisiones de autorización, siguiendo el modelo de evaluación multi-capa.

---
#### **HU-2.1: Andamiaje del Crate `hodei-authorizer` y la Feature `evaluate_permissions`**
*   **Como** arquitecto de software,
*   **quiero** crear la estructura base del crate `hodei-authorizer`,
*   **para** establecer el fundamento sobre el cual se construirán las funcionalidades de autorización.
*   **Justificación:** Es el punto de partida técnico. Define la API pública del nuevo servicio y crea el esqueleto de su primera y más importante feature.
*   **Detalles de Implementación:**
    *   **Acciones:**
        1.  `cargo new crates/hodei-authorizer` y añadirlo al workspace.
        2.  Crear la estructura de directorios VSA para `features/evaluate_permissions`.
    *   **DTOs a Definir en `dto.rs`:**
        *   `struct AuthorizationRequest { principal_hrn: Hrn, action: String, resource_hrn: Hrn, context: serde_json::Value }`
        *   `struct AuthorizationResponse { decision: Decision, determining_policies: Vec<String> }`
        *   `enum Decision { Allow, Deny }`
*   **Criterios de Aceptación:**
    1.  El nuevo crate compila.
    2.  La estructura de la feature `evaluate_permissions` está completa con ficheros vacíos o con contenido básico.

---
#### **HU-2.2: Implementar el Adaptador `IamPolicyProvider` en `hodei-iam`** ✅ **COMPLETADA**
*   **Como** el crate `hodei-iam`,
*   **quiero** proveer una implementación del `IamPolicyProvider`,
*   **para** que el `hodei-authorizer` pueda consumir las políticas de identidad que yo gestiono.
*   **Justificación:** Este es un punto de integración clave. Permite que el autorizador, un servicio agnóstico, obtenga los datos que necesita del dominio de IAM.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/hodei-iam/src/shared/infrastructure/surreal/iam_policy_provider.rs`.
    *   **Lógica/Algoritmo:**
        1.  El método `get_identity_policies_for` recibe un `principal_hrn`.
        2.  Realizar una query a SurrealDB para obtener el `User` con ese `hrn`.
        3.  Si se encuentra, realizar una segunda query para obtener los `Group` a los que pertenece (basado en `user.group_hrns`).
        4.  Para el usuario y cada grupo, realizar queries adicionales para obtener los `Policy` adjuntos.
        5.  Combinar todas las políticas encontradas en un único `PolicySet` de Cedar.
        6.  Devolver el `PolicySet`.
*   **Criterios de Aceptación:**
    1.  ✅ La implementación está completa y el `//TODO` eliminado.
    2.  ✅ Existen tests de integración que verifican que el adaptador devuelve el `PolicySet` correcto para un usuario en varios grupos.
*   **Dependencias:** `HU-2.1` (necesita el trait `IamPolicyProvider` definido allí).

#### **HU-2.3: Implementar la Lógica de Decisión de IAM en `EvaluatePermissionsUseCase`** ✅ **COMPLETADA**
*   **Como** el `EvaluatePermissionsUseCase`,
*   **quiero** usar el motor de Cedar para evaluar las políticas de IAM,
*   **para** determinar si la acción debe ser permitida o denegada según las reglas de identidad.
*   **Justificación:** Implementa el núcleo de la lógica de negocio del autorizador para la capa de IAM.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs`.
    *   **Lógica/Algoritmo:**
        1.  El `execute` recibe el `AuthorizationRequest`.
        2.  Invoca a `self.iam_policy_provider.get_identity_policies_for(...)`.
        3.  Crea un `cedar_policy::Request` a partir del DTO.
        4.  Crea un `cedar_policy::Entities` vacío (por ahora, las entidades de la solicitud se resuelven en el `PolicySet`).
        5.  Llama a `Authorizer::new().is_authorized(...)`.
        6.  Analiza la `Response`:
            *   Si `response.decision() == Decision::Deny`, retornar `AuthorizationResponse { decision: Deny, ... }`.
            *   Si `response.decision() == Decision::Allow`, retornar `AuthorizationResponse { decision: Allow, ... }`.
            *   En cualquier otro caso (implícitamente, si no hay políticas que apliquen), retornar `Deny` (Principio de Privilegio Mínimo).
*   **Criterios de Aceptación:**
    1.  ✅ El caso de uso implementa correctamente la lógica de decisión.
    2.  ✅ Los tests unitarios cubren los tres escenarios: Denegación explícita, Permiso explícito y Denegación implícita.
*   **Dependencias:** `HU-2.2`.

---

### **Epic 3: Integrar Límites Organizacionales (SCPs)**

**Objetivo:** Añadir la capa de validación de SCPs de `hodei-organizations` al flujo de autorización, asegurando que las denegaciones de la organización tengan la máxima prioridad.

---
#### **HU-3.1: Definir el Puerto `OrganizationBoundaryProvider` en el Autorizador**
*   **Como** el `EvaluatePermissionsUseCase`,
*   **necesito** una forma de obtener los SCPs efectivos para la cuenta de un recurso,
*   **para** aplicar los guardarraíles de la organización antes de evaluar los permisos de IAM.
*   **Justificación:** Establece el contrato para que el autorizador pueda consultar los límites organizacionales sin acoplarse al crate `hodei-organizations`.
*   **Detalles de Implementación:**
    *   **Fichero a Crear/Modificar:** `crates/hodei-authorizer/src/features/evaluate_permissions/ports.rs`.
    *   **Trait a Definir:**
        *   `trait OrganizationBoundaryProvider`: Debe ser `async`. Definirá un método `async fn get_effective_scps_for(&self, resource_hrn: &Hrn) -> Result<PolicySet>`. El método recibe el `Hrn` del recurso para poder determinar la cuenta a la que pertenece.
*   **Criterios de Aceptación:**
    1.  El trait `OrganizationBoundaryProvider` está definido en el `ports.rs` del autorizador.
    2.  El código compila sin errores.
*   **Dependencias:** `HU-2.1`.

---
#### **HU-3.2: Implementar el Adaptador `OrganizationBoundaryProvider` en `hodei-organizations`**
*   **Como** el crate `hodei-organizations`,
*   **quiero** proveer una implementación del `OrganizationBoundaryProvider`,
*   **para** que el `hodei-authorizer` pueda consumir los SCPs que yo gestiono.
*   **Justificación:** Conecta el dominio de las organizaciones con el servicio de autorización, permitiendo que las políticas de la organización influyan en las decisiones de permisos.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs`.
    *   **Lógica/Algoritmo:**
        1.  El adaptador `SurrealOrganizationBoundaryProvider` implementará el trait `OrganizationBoundaryProvider`.
        2.  El método `get_effective_scps_for` recibirá el `resource_hrn`.
        3.  Debe implementar la lógica para ascender en la jerarquía desde ese recurso (si es una `Account`) o desde sus `Account` hijas (si es una `OU`) para encontrar la `Account` a la que pertenece. Se puede usar el `HierarchyService` existente.
        4.  Una vez identificada la `Account`, utilizará el caso de uso `GetEffectiveScpsUseCase` para obtener el `PolicySet` de SCPs.
        5.  Devolverá el `PolicySet` resultante.
*   **Criterios de Aceptación:**
    1.  El `SurrealOrganizationBoundaryProvider` implementa completamente el trait.
    2.  El `//TODO` en el fichero es eliminado.
    3.  Existen tests de integración que verifican que el adaptador devuelve los SCPs correctos para un `Hrn` dado.
*   **Dependencias:** `HU-3.1`.

---
#### **HU-3.3: Integrar la Lógica de Evaluación de SCPs en `EvaluatePermissionsUseCase`**
*   **Como** desarrollador,
*   **quiero** que `EvaluatePermissionsUseCase` evalúe los SCPs *antes* que las políticas de IAM,
*   **para** que una denegación de SCP bloquee la solicitud inmediatamente, respetando la lógica de AWS.
*   **Justificación:** Implementa una de las reglas de negocio más críticas del sistema de permisos: la precedencia de los límites organizacionales.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs`.
    *   **Lógica/Algoritmo:**
        1.  Inyectar el `OrganizationBoundaryProvider` en `EvaluatePermissionsUseCase`.
        2.  Refactorizar el método `execute` para que siga este flujo:
            a.  Recibir el `AuthorizationRequest`.
            b.  Llamar a `self.organization_boundary_provider.get_effective_scps_for(&request.resource_hrn)`.
            c.  Evaluar la solicitud (`cedar_policy::Request`) contra el `PolicySet` de SCPs.
            d.  **Punto de Decisión:** Si la respuesta de Cedar es `Deny`, retornar inmediatamente `AuthorizationResponse { decision: Deny, ... }`. La evaluación termina.
            e.  Si la respuesta es `Allow` (o no hay políticas que apliquen), continuar con el flujo existente de evaluación de políticas de IAM (llamada al `IamPolicyProvider`).
*   **Criterios de Aceptación:**
    1.  El `EvaluatePermissionsUseCase` sigue el flujo de evaluación en dos pasos.
    2.  Los tests unitarios se actualizan para incluir escenarios donde un SCP deniega una acción que de otro modo estaría permitida por IAM, y viceversa.
*   **Dependencias:** `HU-3.2`.

---
### **Epic 4: Activar el Análisis Proactivo de Políticas (Access Analyzer)**

**Objetivo:** Exponer la funcionalidad de análisis estático existente en el crate `policies` a través de una API, permitiendo a los usuarios validar sus políticas antes de desplegarlas.

---
#### **HU-4.1: Crear un Endpoint REST para la Feature `policy_analysis`**
*   **Como** ingeniero de seguridad,
*   **quiero** poder enviar un conjunto de políticas a un endpoint `/policies/analyze`,
*   **para** recibir un análisis de posibles violaciones de seguridad y malas prácticas.
*   **Justificación:** Transforma una capacidad interna del crate `policies` en una herramienta de cara al usuario, proporcionando valor de seguridad proactivo.
*   **Detalles de Implementación:**
    *   **Fichero a Crear/Modificar:** `src/api_http/src/api/policies/handlers.rs` (o similar en el crate ejecutable).
    *   **Lógica del Controlador:**
        1.  Definir un handler `analyze_policies_handler` que acepte una petición POST con un cuerpo JSON.
        2.  El cuerpo de la petición debe poder deserializarse en el DTO `AnalyzePoliciesRequest` del crate `policies`.
        3.  Inyectar el `AnalyzePoliciesUseCase` en el estado del handler (ej. Axum `State`).
        4.  Llamar a `use_case.execute(request_dto)`.
        5.  Serializar la `AnalyzePoliciesResponse` resultante en una respuesta HTTP 200 OK.
        6.  Manejar posibles errores del caso de uso y convertirlos en respuestas HTTP apropiadas (ej. 400 Bad Request, 500 Internal Server Error).
*   **Criterios de Aceptación:**
    1.  La ruta `POST /policies/analyze` está registrada en el router de la aplicación.
    2.  Una petición válida con políticas y reglas devuelve un `200 OK` con un JSON de `AnalyzePoliciesResponse`.
    3.  Una petición mal formada devuelve un `400 Bad Request`.
*   **Dependencias:** La feature `policy_analysis` ya existe en el crate `policies`.

---
#### **HU-4.2: Implementar la Regla de Análisis "Sin Wildcard en Recurso"**
*   **Como** administrador de seguridad,
*   **quiero** que el analizador de políticas me advierta si una política aplica a `resource == *`,
*   **para** prevenir permisos que otorgan acceso a todos los recursos de un tipo sin restricciones.
*   **Justificación:** Enriquece la funcionalidad del `Access Analyzer` con una regla de "buenas prácticas" muy común, aumentando su utilidad.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/policies/src/features/policy_analysis/use_case.rs`.
    *   **Lógica/Algoritmo:**
        1.  Añadir un nuevo `match` arm para la regla `"no_resource_wildcard"` dentro de `execute`.
        2.  La lógica no puede ser una simple búsqueda de texto, ya que `resource` es una palabra clave común. Se debe analizar la estructura de la política.
        3.  Una opción es parsear cada política con `cedar_policy::Policy::from_str` y luego inspeccionar su estructura abstracta (AST) para encontrar condiciones como `resource == *` o cláusulas `principal, action, resource` sin condiciones adicionales sobre el recurso.
        4.  Si se encuentra una política que viola esta regla, se añade una `RuleViolation` a la respuesta.
*   **Criterios de Aceptación:**
    1.  Al invocar el `AnalyzePoliciesUseCase` con una política que contiene `resource` sin restricciones específicas y la regla activada, se devuelve una violación.
    2.  Políticas que restringen el recurso (ej. `resource == MyResource::"id"`, `resource in MyOrg::"resources"`) no disparan la violación.
*   **Dependencias:** `HU-4.1`.

---
### **Epic 5: Habilitar Auditoría y Trazabilidad (CloudTrail)**

**Objetivo:** Crear un rastro de auditoría inmutable para cada decisión de autorización, proporcionando visibilidad completa sobre quién accedió a qué y por qué.

---
#### **HU-5.1: Definir el Trait `AuditLogger` y el Struct `AuditEvent` en el Shared Kernel**
*   **Como** desarrollador,
*   **quiero** un contrato claro (`AuditLogger`) y un modelo de datos (`AuditEvent`) para la auditoría,
*   **para** desacoplar la lógica de autorización de la implementación específica de logging.
*   **Justificación:** Es el primer paso para una auditoría robusta. Define el "qué" se va a registrar, antes de decidir el "cómo" y el "dónde".
*   **Detalles de Implementación:**
    *   **Fichero a Crear:** `crates/shared/src/auditing.rs`.
    *   **Struct `AuditEvent`:**
        *   Debe contener campos como `timestamp`, `principal_hrn`, `action`, `resource_hrn`, `decision` (Allow/Deny), `determining_policies` (Vec<String>), `context` (JSON), `source_ip`, `user_agent`, etc.
    *   **Trait `AuditLogger`:**
        *   Definirá un método `async fn log_decision(&self, event: AuditEvent) -> Result<(), AuditError>`.
*   **Criterios de Aceptación:**
    1.  El módulo `auditing.rs` existe en `crates/shared`.
    2.  El struct `AuditEvent` y el trait `AuditLogger` están definidos y son públicos.

---
#### **HU-5.2: Integrar el `AuditLogger` en el `EvaluatePermissionsUseCase`**
*   **Como** el `EvaluatePermissionsUseCase`,
*   **quiero** registrar el resultado de cada evaluación de permisos,
*   **para** cumplir con los requisitos de auditoría del sistema.
*   **Justificación:** Conecta la lógica de decisión con el sistema de auditoría, asegurando que ninguna decisión pase sin ser registrada.
*   **Detalles de Implementación:**
    *   **Fichero a Modificar:** `crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs`.
    *   **Lógica/Algoritmo:**
        1.  Inyectar `Arc<dyn AuditLogger>` en el `EvaluatePermissionsUseCase`.
        2.  Justo antes de cada `return` en el método `execute` (tanto para `Allow` como para `Deny`), construir un `AuditEvent` con toda la información disponible de la solicitud y la respuesta.
        3.  Llamar a `self.audit_logger.log_decision(event).await`.
        4.  La llamada al logger debe ser a prueba de fallos; un error en el logging no debe hacer fallar la solicitud de autorización principal. Se puede usar `tokio::spawn` para hacerlo en segundo plano o simplemente registrar el error de auditoría sin propagarlo.
*   **Criterios de Aceptación:**
    1.  El `EvaluatePermissionsUseCase` tiene una nueva dependencia: `AuditLogger`.
    2.  Se realiza una llamada a `log_decision` para cada ruta de salida del método `execute`.
*   **Dependencias:** `HU-5.1`.

---
#### **HU-5.3: Implementar un `SurrealAuditLogger` para la Persistencia de Auditoría**
*   **Como** operador del sistema,
*   **quiero** que los eventos de auditoría se almacenen de forma persistente en SurrealDB,
*   **para** poder consultarlos y analizarlos posteriormente.
*   **Justificación:** Proporciona la implementación concreta que guarda los datos de auditoría, haciéndolos útiles y duraderos.
*   **Detalles de Implementación:**
    *   **Fichero a Crear:** `crates/shared/src/infrastructure/surreal_audit_logger.rs` (o en un crate de infraestructura dedicado).
    *   **Lógica/Algoritmo:**
        1.  Crear la struct `SurrealAuditLogger` que contiene una conexión a SurrealDB.
        2.  Implementar el trait `AuditLogger`.
        3.  El método `log_decision` tomará el `AuditEvent` y lo insertará como un nuevo registro en una tabla llamada `audit_log`. La tabla puede usar un ID de SurrealDB autogenerado.
*   **Criterios de Aceptación:**
    1.  La implementación existe y se conecta a SurrealDB.
    2.  Cuando se llama a `log_decision`, se crea un nuevo documento en la tabla `audit_log` con los datos del evento.
    3.  El DI global en `api_http` está configurado para proveer `SurrealAuditLogger` como la implementación de `AuditLogger`.
*   **Dependencias:** `HU-5.2`.
