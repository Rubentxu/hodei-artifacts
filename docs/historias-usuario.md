### Revisión de Código Actualizada

#### 1. Análisis General

Tras la aclaración, el código demuestra una arquitectura interna de Bounded Context más robusta de lo que parecía inicialmente. El patrón de tener un núcleo de dominio (`core`/`internal`) compartido por las features del `crate` es pragmático y correcto. La adopción del nombre `internal` y la privatización del módulo han resuelto las principales inconsistencias de encapsulamiento, lo que representa un gran paso adelante. La estructura VSA y el uso de puertos segregados se mantienen como puntos fuertes. Sin embargo, persisten algunas inconsistencias críticas, como la implementación ficticia del patrón Unit of Work y la fuga de un adaptador en la API pública de `hodei-organizations`. Además, la existencia de *feature slices* monolíticas para operaciones CRUD sigue siendo un área de mejora para alinear el código completamente con la VSA.

#### 2. Puntos Fuertes

-   **Excelente Encapsulamiento del Bounded Context:** Con el cambio de `shared` a `internal` y su declaración como módulo privado (`mod internal;`), el `crate` ahora impone un fuerte encapsulamiento. Los detalles de implementación como entidades de dominio y repositorios están correctamente protegidos, haciendo que el compilador garantice los límites arquitectónicos.
-   **Estructura VSA por Feature:** La mayoría de las features siguen una estructura de "rebanada vertical" clara y cohesiva.
-   **Segregación de Interfaces (ISP):** Se definen puertos (`traits`) específicos para cada caso de uso, promoviendo un bajo acoplamiento y alta cohesión.
-   **Inyección de Dependencias:** El código depende de abstracciones, lo que facilita las pruebas y la flexibilidad.
-   **Tests Unitarios Sólidos:** Los `use_case_test.rs` demuestran un claro enfoque en probar la lógica de negocio de forma aislada mediante el uso de mocks.
-   **APIs basadas en DTOs:** Las interfaces públicas de los casos de uso están bien definidas mediante DTOs, evitando la fuga de entidades de dominio.

---

#### 3. Críticas, Inconsistencias y Mejoras (Punto por Punto)

1.  **Implementación Ficticia de Unit of Work (UoW)**
  -   **Identificar el Problema:** `crates/hodei-iam/src/features/add_user_to_group/adapter.rs` (y similares en `create_user`, `create_group`).
  -   **Describir la Inconsistencia:** La implementación `Generic...UnitOfWork` no gestiona una transacción real de base de datos. Utiliza un `std::sync::Mutex<bool>` para simular el estado de una transacción.
  -   **Explicar el Impacto:** Esto es extremadamente peligroso en un entorno de producción. A pesar de que el código *parece* transaccional, no ofrece ninguna garantía de atomicidad. Si una operación de guardado falla a mitad del caso de uso, el sistema quedará en un estado inconsistente sin posibilidad de rollback real. Esto rompe la fiabilidad de las operaciones que modifican múltiples agregados.
  -   **Proponer una Solución Concreta:** La implementación del UoW debe delegar en el mecanismo de transacciones real de la base de datos subyacente (p. ej., SurrealDB). El adaptador debe iniciar, confirmar o revertir una transacción de base de datos real.

    ```rust
    // Ejemplo conceptual para un UoW real con SurrealDB
    // Fichero: crates/hodei-organizations/src/internal/infrastructure/surreal/unit_of_work.rs
    
    pub struct SurrealUnitOfWork {
        // El cliente de DB se pasa en el constructor.
        // La transacción se inicia en `begin`.
        db: Surreal<Any>,
        // Podría contener un objeto de transacción si el driver lo soporta.
    }

    #[async_trait]
    impl UnitOfWork for SurrealUnitOfWork {
        async fn begin(&mut self) -> Result<(), UnitOfWorkError> {
            // Inicia una transacción REAL en la base de datos.
            self.db.query("BEGIN TRANSACTION;").await?;
            Ok(())
        }
    
        async fn commit(&mut self) -> Result<(), UnitOfWorkError> {
            self.db.query("COMMIT TRANSACTION;").await?;
            Ok(())
        }
    
        async fn rollback(&mut self) -> Result<(), UnitOfWorkError> {
            self.db.query("CANCEL TRANSACTION;").await?;
            Ok(())
        }
        
        // ... los métodos que devuelven repositorios ahora usarían `self.db`.
    }
    ```

2.  **Feature Slice Monolítica para CRUD**
  -   **Identificar el Problema:** `crates/hodei-iam/src/features/create_policy/use_case.rs` y `ports.rs`.
  -   **Describir la Inconsistencia:** El módulo `create_policy` en realidad maneja `Create`, `Delete`, `Update`, `Get` y `List`. Esto viola la **Regla #4 (VSA por Feature)**.
  -   **Explicar el Impacto:** El puerto `PolicyPersister` se vuelve demasiado grande (violando ISP) y la cohesión del módulo disminuye.
  -   **Proponer una Solución Concreta:** Dividir el módulo en features individuales: `create_policy`, `delete_policy`, `update_policy`, `get_policy` y `list_policies`, cada una con su propia estructura VSA y su puerto segregado.

3.  **Fuga de un Adaptador en la API Pública del Crate**
  -   **Identificar el Problema:** `crates/hodei-organizations/src/lib.rs`.
  -   **Describir la Inconsistencia:** El `crate` exporta públicamente `GetEffectiveScpsAdapter`. Un adaptador es un detalle de implementación de la capa de infraestructura. La API pública de un Bounded Context solo debe exponer sus capacidades (casos de uso) y los DTOs asociados.
  -   **Explicar el Impacto:** Esto crea un acoplamiento indebido. El `crate` consumidor (`hodei-authorizer`) no solo necesita saber sobre el puerto (`GetEffectiveScpsPort`), sino también sobre su implementación específica (`GetEffectiveScpsAdapter`). La construcción y el cableado de adaptadores deben ocurrir en la "composition root" de la aplicación (p. ej., en `main.rs`), no dentro de la biblioteca.
  -   **Proponer una Solución Concreta:** Eliminar la exportación pública del adaptador. El `crate` `hodei-organizations` debe exponer el puerto y una función constructora para el caso de uso. El `crate` `hodei-authorizer` dependerá del puerto, y la aplicación principal se encargará de crear el adaptador y realizar la inyección.

    ```rust
    // Antes (en crates/hodei-organizations/src/lib.rs)
    pub struct GetEffectiveScpsAdapter<...> { ... }
    pub use ... GetEffectiveScpsAdapter;

    // Después (en crates/hodei-organizations/src/lib.rs)
    // El struct GetEffectiveScpsAdapter y su implementación se mueven al crate de aplicación
    // (o a un crate "glue" de infraestructura), NO se exponen aquí.
    // Lo que sí se expone es el caso de uso y su constructor.
    pub use features::get_effective_scps::{
        GetEffectiveScpsUseCase, 
        di::get_effective_scps_use_case // Un constructor para el caso de uso.
    };

    // En main.rs o composition root:
    // 1. Crear repositorios de `hodei-organizations`.
    // 2. Usar `get_effective_scps_use_case` para crear la instancia del caso de uso.
    // 3. Crear el `GetEffectiveScpsAdapter` envolviendo el caso de uso.
    // 4. Inyectar el adaptador (como `Arc<dyn GetEffectiveScpsPort>`) en `hodei-authorizer`.
    ```



---

### Historias de Usuario Completas y Actualizadas

#### Bounded Context: IAM (`hodei-iam`)

**HU-IAM-001: Creación de un nuevo usuario**
*   **Como** un administrador del sistema (o un servicio de API),
*   **quiero** crear un nuevo usuario proporcionando su nombre y correo electrónico,
*   **para** poder registrar nuevos individuos en el sistema.
- **Estado:** ✅ Completada (use case `CreateUserUseCase` y test `crates/hodei-iam/tests/integration_create_user_comprehensive_test.rs`)
*   **AC:**
      1.  El sistema debe generar un HRN único y global para el nuevo usuario.
      2.  El usuario se debe persistir en la base de datos de forma transaccional.
      3.  Tras la creación exitosa, se debe emitir un evento de dominio `UserCreated`.
      4.  **[Test de Integración]** Debe existir un test de integración que invoque el caso de uso `CreateUserUseCase` a través de la API pública del `crate` (`hodei_iam::features::create_user::di::make_use_case`), usando un repositorio en memoria (`InMemoryUserRepository`) y verifique que el usuario se crea correctamente sin acceder a ningún módulo `internal`.

**HU-IAM-002: Creación de un nuevo grupo**
*   **Como** un administrador del sistema,
*   **quiero** crear un nuevo grupo de usuarios proporcionando un nombre,
*   **para** poder agrupar usuarios con permisos similares.
- **Estado:** ✅ Completada (use case `CreateGroupUseCase` y test `crates/hodei-iam/tests/integration_create_group_comprehensive_test.rs`)
*   **AC:**
      1.  El sistema debe generar un HRN único para el nuevo grupo.
      2.  El grupo se debe persistir en la base de datos de forma transaccional.
      3.  Tras la creación exitosa, se debe emitir un evento de dominio `GroupCreated`.
      4.  **[Test de Integración]** Debe existir un test de integración que utilice la API pública del `crate` (`make_create_group_uc`) con un repositorio en memoria para confirmar la creación del grupo y la correcta formación del DTO de respuesta (`GroupView`).

**HU-IAM-003: Añadir un usuario a un grupo**
*   **Como** un administrador del sistema,
*   **quiero** añadir un usuario existente a un grupo existente,
*   **para** que el usuario herede los permisos asociados a ese grupo.
- **Estado:** 🟡 En progreso (la UoW actual `GenericAddUserToGroupUnitOfWork` simula la transacción y sigue pendiente implementar soporte transaccional real)
*   **AC:**
      1.  La operación debe ser atómica y transaccional, garantizada por un Unit of Work.
      2.  El sistema debe verificar que tanto el usuario como el grupo existen antes de proceder.
      3.  La operación debe ser idempotente.
      4.  Tras la asignación exitosa, se debe emitir un evento `UserAddedToGroup`.
      5.  **[Test de Integración]** Debe existir un test que, usando la API pública, cree un usuario y un grupo, llame al `AddUserToGroupUseCase`, y verifique que la entidad `User` fue actualizada correctamente.

**HU-IAM-004: Creación de una política IAM**
*   **Como** un administrador de seguridad,
*   **quiero** crear una nueva política IAM proporcionando su contenido en lenguaje Cedar,
*   **para** definir un conjunto de permisos reutilizable.
- **Estado:** ✅ Completada (feature `create_policy_new` con tests unitarios e integración en `crates/hodei-iam/tests/integration_create_policy_new_test.rs`)
*   **AC:**
      1.  El contenido de la política debe ser validado sintácticamente.
      2.  Si la política es válida, se debe persistir y asignar un HRN único.
      3.  **[Test de Integración]** Debe existir un test que utilice el `CreatePolicyUseCase` con mocks para sus puertos y valide la lógica del caso de uso.

**HU-IAM-005: Consultar políticas efectivas de un principal**
*   **Como** un servicio de autorización,
*   **quiero** solicitar todas las políticas IAM efectivas para un principal,
*   **para** poder tomar una decisión de autorización.
- **Estado:** 🟡 En progreso (existe `GetEffectivePoliciesForPrincipalUseCase`, pero falta test de integración ejercitando la API pública)
*   **AC:**
      1.  La respuesta debe incluir las políticas directamente adjuntas al usuario y las heredadas de sus grupos.
      2.  La API pública debe devolver las políticas como un `Vec<String>`, sin exponer entidades internas.
      3.  **[Test de Integración]** Debe existir un test que use la API pública del `GetEffectivePoliciesForPrincipalUseCase` con mocks para sus puertos, simulando diferentes escenarios de herencia y verificando la agregación de políticas.

**HU-IAM-006: Leer una política IAM**
*   **Como** un administrador,
*   **quiero** obtener los detalles de una política IAM por su HRN,
*   **para** revisar su contenido y descripción.
- **Estado:** ⏳ Pendiente
*   **AC:**
      1.  La API debe devolver un DTO (`PolicyDto`) con los detalles de la política.
      2.  Si la política no existe, se debe devolver un error `PolicyNotFound`.
      3.  **[Test de Integración]** Debe existir un test que use el `GetPolicyUseCase` público para recuperar una política creada previamente (usando un persister en memoria).

**HU-IAM-007: Actualizar una política IAM**
*   **Como** un administrador,
*   **quiero** actualizar el contenido o la descripción de una política IAM existente,
*   **para** modificar sus permisos.
- **Estado:** ⏳ Pendiente (faltan `use_case.rs`, adaptadores y pruebas)
*   **AC:**
      1.  El nuevo contenido de la política debe ser validado sintácticamente.
      2.  La operación debe ser atómica.
      3.  Si la política no existe, se debe devolver un error `PolicyNotFound`.
      4.  **[Test de Integración]** Debe existir un test que use el `UpdatePolicyUseCase` público para modificar una política y verifique que el cambio se ha persistido.

**HU-IAM-008: Borrar una política IAM**
*   **Como** un administrador,
*   **quiero** borrar una política IAM que ya no se utiliza,
*   **para** mantener el sistema limpio y seguro.
- **Estado:** 🟡 En progreso (feature `delete_policy` implementada, falta validar adaptadores reales e integración end-to-end)
*   **AC:**
      1.  El sistema debería (idealmente) verificar que la política no está adjunta a ningún principal antes de borrarla.
      2.  Si la política no existe, se debe devolver un error `PolicyNotFound`.
      3.  **[Test de Integración]** Debe existir un test que use el `DeletePolicyUseCase` público para borrar una política y verifique que ya no se puede recuperar.

**HU-IAM-009: Listar políticas IAM**
*   **Como** un administrador,
*   **quiero** listar todas las políticas IAM disponibles, con opción de paginación,
*   **para** tener una visión general de los permisos definidos.
- **Estado:** ⏳ Pendiente
*   **AC:**
      1.  La API debe soportar parámetros de `limit` y `offset` para la paginación.
      2.  La respuesta debe ser una lista de DTOs (`Vec<PolicyDto>`).
      3.  **[Test de Integración]** Debe existir un test que cree varias políticas y luego use el `ListPoliciesUseCase` público para verificar que la paginación (`limit` y `offset`) funciona como se espera.

---

#### Bounded Context: Organizations (`hodei-organizations`)

**HU-ORG-001: Creación de una nueva cuenta**
*   **Como** un administrador de la organización,
*   **quiero** crear una nueva cuenta bajo una Unidad Organizativa (OU) específica,
*   **para** aislar recursos y facturación.
- **Estado:** 🟡 En progreso (use case y tests unitarios listos, falta validar adaptador real y prueba de integración)
*   **AC:**
      1.  Se debe generar un HRN único para la cuenta.
      2.  La operación debe ser transaccional.
      3.  Se debe emitir un evento `AccountCreated`.
      4.  **[Test de Integración]** Debe existir un test que use la API pública (`make_create_account_uc`) con un UoW en memoria para validar la creación.

**HU-ORG-002: Creación de una nueva Unidad Organizativa (OU)**
*   **Como** un administrador de la organización,
*   **quiero** crear una nueva OU bajo una OU padre existente o en la raíz,
*   **para** estructurar jerárquicamente mis cuentas.
- **Estado:** 🟡 En progreso (feature `create_ou` completa en lógica y tests unitarios, falta cobertura de integración)
*   **AC:**
      1.  Se debe validar que la OU padre existe (si se proporciona).
      2.  La operación debe ser transaccional.
      3.  **[Test de Integración]** Debe existir un test que use la API pública para crear una jerarquía de OUs y valide que las entidades se persisten correctamente.

**HU-ORG-003: Mover una cuenta entre OUs**
*   **Como** un administrador de la organización,
*   **quiero** mover una cuenta de una OU de origen a una de destino de forma atómica,
*   **para** reflejar cambios organizacionales sin riesgo de estados inconsistentes.
- **Estado:** 🟡 En progreso (use case `MoveAccountUseCase` implementado con UoW, falta integración end-to-end y validación con infra real)
*   **AC:**
      1.  La operación debe ser **atómica y transaccional**.
      2.  Se debe verificar la existencia de la cuenta y de ambas OUs.
      3.  Se debe emitir un evento `AccountMoved`.
      4.  **[Test de Integración]** Debe existir un test que configure un estado inicial, invoque el `MoveAccountUseCase` a través de su API pública, y verifique el estado final, confirmando la atomicidad.

**HU-ORG-004: Creación de una Política de Control de Servicio (SCP)**
*   **Como** un administrador de la organización,
*   **quiero** crear una nueva SCP con contenido Cedar,
*   **para** definir barreras de permisos a nivel organizacional.
- **Estado:** 🟡 En progreso (vertical slice `create_scp` lista, adaptadores reales pendientes)
*   **AC:**
      1.  El contenido de la SCP debe ser validado sintácticamente.
      2.  La SCP se debe persistir con un HRN único.
      3.  **[Test de Integración]** Debe existir un test que use la API pública del `CreateScpUseCase` para validar la creación.

**HU-ORG-005: Adjuntar una SCP a una cuenta o a una OU**
*   **Como** un administrador de la organización,
*   **quiero** adjuntar una SCP existente a una cuenta o a una OU,
*   **para** aplicar las barreras de permisos definidas en la SCP.
- **Estado:** 🟡 En progreso (feature `attach_scp` con lógica y tests unitarios, falta validación contra storage real)
*   **AC:**
      1.  Se debe verificar la existencia de la SCP y del objetivo.
      2.  La operación debe ser transaccional.
      3.  Se debe emitir un evento `ScpAttached`.
      4.  **[Test de Integración]** Debe existir un test que, usando la API pública, cree una SCP y una OU, llame al `AttachScpUseCase`, y verifique que la OU fue actualizada.

**HU-ORG-006: Consultar SCPs efectivas para una entidad**
*   **Como** un servicio de autorización,
*   **quiero** obtener todas las SCPs efectivas que se aplican a una entidad (cuenta o OU),
*   **para** evaluar las barreras de permisos organizacionales.
- **Estado:** 🟡 En progreso (use case `GetEffectiveScpsUseCase` operativo con tests unitarios, falta integración que produzca `PolicySet` completo)
*   **AC:**
      1.  La API pública debe devolver un `PolicySet` de Cedar.
      2.  La respuesta debe incluir las SCPs de la entidad y de toda su jerarquía de padres.
      3.  **[Test de Integración]** Debe existir un test que construya una jerarquía, adjunte SCPs, y use el `GetEffectiveScpsUseCase` (a través de su puerto) para verificar la lógica de agregación.

**HU-ORG-007: Leer una SCP** (Análoga a HU-IAM-006)
*   **Como** un administrador, **quiero** obtener los detalles de una SCP por su HRN.
*   **AC:** Devolver un DTO `ScpDto` o `ScpNotFound`. Test de integración.
- **Estado:** ⏳ Pendiente

**HU-ORG-008: Actualizar una SCP** (Análoga a HU-IAM-007)
*   **Como** un administrador, **quiero** actualizar el contenido de una SCP existente.
*   **AC:** Validar nuevo contenido, operación atómica, error si no existe. Test de integración.
- **Estado:** ⏳ Pendiente

**HU-ORG-009: Borrar una SCP** (Análoga a HU-IAM-008)
*   **Como** un administrador, **quiero** borrar una SCP que no esté adjunta a ninguna entidad.
*   **AC:** Verificar que no esté en uso, error si no existe. Test de integración.
- **Estado:** ⏳ Pendiente

**HU-ORG-010: Listar SCPs** (Análoga a HU-IAM-009)
*   **Como** un administrador, **quiero** listar todas las SCPs disponibles con paginación.
*   **AC:** Soportar `limit`/`offset`. Test de integración.
- **Estado:** ⏳ Pendiente

---

#### Bounded Context: Authorizer (`hodei-authorizer`)

**HU-AUTH-001: Evaluar una solicitud de autorización completa**
*   **Como** un microservicio o API Gateway,
*   **quiero** preguntar si un principal tiene permiso para realizar una acción sobre un recurso,
*   **para** proteger el acceso a los recursos del sistema de forma centralizada.
- **Estado:** 🟡 En progreso (use case `EvaluatePermissionsUseCase` operativo con pruebas unitarias, depende de completar evaluadores IAM/SCP para cumplir AC)
*   **AC:**
      1.  La decisión final debe ser `Deny` si CUALQUIER SCP efectiva deniega la acción (principio de "Deny by default").
      2.  Si las SCPs lo permiten, la decisión final debe basarse en las políticas IAM efectivas del principal.
      3.  Si no hay ninguna política IAM que permita explícitamente la acción, la decisión es `Deny`.
      4.  La decisión final, la razón y las métricas de evaluación deben ser registradas.
      5.  Los resultados deben ser cacheados para optimizar el rendimiento.
      6.  **[Test de Integración]** Debe existir un test que construya el `EvaluatePermissionsUseCase` inyectando *mocks* para los puertos `EffectivePoliciesQueryPort` y `GetEffectiveScpsPort`, y valide todos los escenarios de decisión (IAM permite/deniega vs. SCP permite/deniega).