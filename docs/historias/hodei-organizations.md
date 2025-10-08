### **Documento de Diseño e Historias de Usuario: Crate `hodei-organizations`**

**Visión General del Crate:**

`hodei-organizations` gestiona la estructura jerárquica de la organización. Sus responsabilidades son:

1.  **Gestionar Entidades de Dominio:** `Account` y `OrganizationalUnit` (OUs), que son `internal`.
2.  **Gestionar Políticas de Control de Servicio (SCPs):** CRUD de `HodeiPolicy` que actúan como barreras de permisos.
3.  **Gestionar la Jerarquía:** Crear OUs, crear Cuentas dentro de OUs, y mover Cuentas entre OUs.
4.  **Exponer la Jerarquía:** Proveer un `UseCase` para que `hodei-authorizer` pueda obtener las SCPs efectivas para cualquier recurso en la jerarquía.

**Dependencias Principales:** `kernel`, `hodei-policies` (para validación de SCPs), `async-trait`, `thiserror`, `serde`, `surrealdb`.

---

### **Épica 1: Re-estructuración y Gestión de la Jerarquía**

#### **HU-ORG-R-000: Implementar la Nueva Estructura de Módulos (ISP Puro)**
*   **Como:** Arquitecto del sistema.
*   **Quiero:** Reorganizar `hodei-organizations` para separar `features` con puertos segregados, `infrastructure` pública y un `internal` sellado.
*   **Para:** Lograr una arquitectura encapsulada y testable.
*   **Criterios de Aceptación:**
    1.  El `crate` tendrá la estructura V7.0: `api.rs`, `features/`, `infrastructure/`, `internal/`.
    2.  `internal/` solo contendrá `domain/` con las `structs` `Account` y `OrganizationalUnit` (`pub(crate)`).
    3.  `infrastructure/` contendrá los adaptadores públicos, como `SurrealOrganizationAdapter`.
    4.  El `api.rs` exportará los módulos `features` e `infrastructure`.

#### **HU-ORG-R-001: Crear una Nueva Unidad Organizativa (OU)**
*   **Como:** Administrador de la organización.
*   **Quiero:** Crear una nueva OU dentro de una OU padre existente (o en la raíz).
*   **Para:** Estructurar mi organización jerárquicamente.
*   **`features/create_ou/`:**
    *   **`ports.rs`:**
        ```rust
        #[async_trait]
        pub trait OuPersister: Send + Sync {
            // Verifica si el padre existe y guarda la nueva OU
            async fn save_ou(&self, ou: &OrganizationalUnit) -> Result<(), CreateOuError>;
        }
        pub trait HrnGenerator { fn new_ou_hrn(&self, name: &str) -> Hrn; }
        ```
    *   **`use_case.rs`:**
        *   `CreateOuUseCase` depende de `OuPersister` y `HrnGenerator`.
        *   **Algoritmo `execute`:**
            1.  Recibe `CreateOuCommand { name, parent_hrn }`.
            2.  Genera un nuevo `Hrn` para la OU.
            3.  Crea la entidad interna `OrganizationalUnit::new(hrn, name, parent_hrn)`.
            4.  Llama a `self.persister.save_ou(&ou)`. El persister es responsable de validar que el `parent_hrn` existe.
            5.  Mapea la entidad a un DTO `OuView` y lo devuelve.

#### **HU-ORG-R-002: Crear una Nueva Cuenta**
*   **Como:** Administrador de la organización.
*   **Quiero:** Crear una nueva cuenta de miembro dentro de una OU.
*   **Para:** Añadir nuevas unidades de negocio o entornos aislados a mi organización.
*   **`features/create_account/`:**
    *   **`ports.rs`:**
        ```rust
        #[async_trait]
        pub trait AccountPersister: Send + Sync {
            // Verifica si el padre (OU) existe y guarda la nueva cuenta
            async fn save_account(&self, account: &Account) -> Result<(), CreateAccountError>;
        }
        pub trait HrnGenerator { fn new_account_hrn(&self, name: &str) -> Hrn; }
        ```
    *   **`use_case.rs`:** Sigue el mismo patrón que `CreateOuUseCase`.

#### **HU-ORG-R-003: Mover una Cuenta entre OUs**
*   **Como:** Administrador de la organización.
*   **Quiero:** Mover una cuenta de una OU de origen a una de destino.
*   **Para:** Reorganizar mi estructura empresarial.
*   **`features/move_account/`:**
    *   **`ports.rs`:**
        ```rust
        #[async_trait]
        pub trait HierarchyManager: Send + Sync {
            // Este método encapsula la operación transaccional
            async fn move_account(&self, account_hrn: &Hrn, source_ou_hrn: &Hrn, target_ou_hrn: &Hrn) -> Result<(), MoveAccountError>;
        }
        ```
    *   **`use_case.rs`:**
        *   `MoveAccountUseCase` depende de `HierarchyManager`.
        *   Su `execute` es un simple _passthrough_ que llama a `self.manager.move_account(...)`.
    *   **`infrastructure/surreal/organization_adapter.rs`:**
        *   El `SurrealOrganizationAdapter` implementará el `trait HierarchyManager`.
        *   **Algoritmo `move_account`:**
            1.  Iniciar una transacción en SurrealDB: `BEGIN TRANSACTION`.
            2.  `SELECT * FROM account WHERE hrn = $account_hrn`. Si no existe, `CANCEL` y devolver error.
            3.  `UPDATE ou SET child_accounts -= $account_hrn WHERE hrn = $source_ou_hrn`.
            4.  `UPDATE ou SET child_accounts += $account_hrn WHERE hrn = $target_ou_hrn`.
            5.  `UPDATE account SET parent_hrn = $target_ou_hrn WHERE hrn = $account_hrn`.
            6.  Si todo va bien, `COMMIT TRANSACTION`. Si algo falla, `CANCEL TRANSACTION`.

---

### **Épica 2: Gestión de Políticas de Control de Servicio (SCPs)**

#### **HU-ORG-R-004: Crear una Nueva SCP**
*   **Como:** Administrador de políticas.
*   **Quiero:** Crear una nueva SCP proveyendo un nombre y un documento de política (`String`).
*   **Para:** Definir barreras de permisos para mi organización.
*   **`features/create_scp/`:**
    *   **`ports.rs`:**
        ```rust
        // Reutiliza el mismo patrón de `hodei-iam`
        pub trait ScpValidator: Send + Sync { ... }
        pub trait ScpPersister: Send + Sync {
            async fn save_scp(&self, scp: &HodeiPolicy) -> Result<(), CreateScpError>;
        }
        pub trait HrnGenerator { fn new_scp_hrn(&self, name: &str) -> Hrn; }
        ```
    *   **`use_case.rs`:**
        *   Similar a `create_policy` en `hodei-iam`. Valida el contenido (delegando a `hodei-policies` a través del puerto `ScpValidator`), crea un `HodeiPolicy`, y lo persiste.

#### **HU-ORG-R-005: Adjuntar una SCP a un Objetivo (OU o Cuenta)**
*   **Como:** Administrador.
*   **Quiero:** Adjuntar una SCP existente a una OU o a una cuenta.
*   **Para:** Aplicar las barreras de permisos definidas en la SCP a una parte de mi organización.
*   **`features/attach_scp/`:**
    *   **`ports.rs`:**
        ```rust
        #[async_trait]
        pub trait ScpAttacher: Send + Sync {
            async fn attach_scp(&self, scp_hrn: &Hrn, target_hrn: &Hrn) -> Result<(), AttachScpError>;
        }
        ```
    *   **`use_case.rs`:**
        *   `AttachScpUseCase` depende de `ScpAttacher`.
        *   **Algoritmo `execute`:**
            1.  Recibe `AttachScpCommand { scp_hrn, target_hrn }`.
            2.  Delega la operación en `self.attacher.attach_scp(...)`.
    *   **`infrastructure/surreal/organization_adapter.rs`:**
        *   `SurrealOrganizationAdapter` implementa `ScpAttacher`.
        *   **Algoritmo `attach_scp`:**
            1.  Determina el tipo de `target_hrn` (Account u OU) por su prefijo.
            2.  Verifica que tanto la SCP como el objetivo existen en la DB.
            3.  Ejecuta una consulta de SurrealDB para crear la relación, p. ej., `RELATE $scp_id->attaches_to->$target_id`. O si se desnormaliza, `UPDATE $target_id SET attached_scps += $scp_hrn`.

---

### **Épica 3: Exponer Capacidades de Autorización**

#### **HU-ORG-R-006: Obtener las SCPs Efectivas para un Recurso**
*   **Como:** El `crate` `hodei-authorizer`.
*   **Quiero:** Solicitar el `HodeiPolicySet` de todas las SCPs que aplican a un recurso (`Account` u `OU`).
*   **Para:** Conocer las barreras de permisos que gobiernan una petición de acceso.
*   **`features/get_effective_scps/`:**
    *   **`ports.rs`:**
        ```rust
        #[async_trait]
        pub trait EffectiveScpsProvider: Send + Sync {
            async fn get_scps_for_resource(&self, resource_hrn: &Hrn) -> Result<HodeiPolicySet, GetEffectiveScpsError>;
        }
        ```
    *   **`use_case.rs`:**
        *   `GetEffectiveScpsUseCase` depende de `EffectiveScpsProvider` y actúa como un _passthrough_.
    *   **`infrastructure/surreal/organization_adapter.rs`:**
        *   `SurrealOrganizationAdapter` implementa `EffectiveScpsProvider`.
        *   **Este es el algoritmo complejo, reutilizado del `organization_boundary_provider` anterior:**
            1.  Recibe el `resource_hrn` de entrada.
            2.  Determina si es una `Account` o una `OU`.
            3.  Inicia un `HodeiPolicySet` vacío y un `HashSet` para detectar ciclos.
            4.  **Bucle de Ascenso:**
                a. Carga la entidad actual (Cuenta u OU) desde la DB.
                b. Si no existe, para.
                c. Añade el `Hrn` de la entidad al `HashSet` de visitados. Si ya estaba, es un ciclo -> error.
                d. Carga todas las SCPs (`HodeiPolicy`) adjuntas a esta entidad y las añade al `HodeiPolicySet`.
                e. Obtiene el `parent_hrn` de la entidad actual.
                f. Si no hay padre, o si el padre es él mismo (raíz), termina el bucle.
                g. Si hay padre, lo convierte en la entidad actual y repite el bucle.
            5.  Devuelve el `HodeiPolicySet` acumulado.

---

### **Implementación Concreta de `infrastructure`**

#### **`crates/hodei-organizations/src/infrastructure/surreal/organization_adapter.rs`**

Este es el artefacto central de la capa de infraestructura. Implementará múltiples `traits`.

```rust
use std::sync::Arc;
use async_trait::async_trait;
use surrealdb::{Surreal, engine::any::Any};

// Importar todos los traits de puerto de las features
use crate::api::create_ou::ports::OuPersister;
use crate::api::create_account::ports::AccountPersister;
use crate::api::move_account::ports::HierarchyManager;
use crate::api::attach_scp::ports::ScpAttacher;
use crate::api::get_effective_scps::ports::EffectiveScpsProvider;
// ... y todos los demás ...

use crate::internal::domain::{Account, OrganizationalUnit};
use kernel::Hrn;
use kernel::domain::policy::HodeiPolicy;

// La implementación concreta pública
pub struct SurrealOrganizationAdapter {
    db: Arc<Surreal<Any>>,
}

impl SurrealOrganizationAdapter {
    pub fn new(db: Arc<Surreal<Any>>) -> Self { Self { db } }
}

// Implementación del puerto para `create_ou`
#[async_trait]
impl OuPersister for SurrealOrganizationAdapter {
    async fn save_ou(&self, ou: &OrganizationalUnit) -> Result<(), ...> {
        // Lógica de SurrealDB para verificar padre y crear la OU
    }
}

// Implementación del puerto para `create_account`
#[async_trait]
impl AccountPersister for SurrealOrganizationAdapter {
    async fn save_account(&self, account: &Account) -> Result<(), ...> {
        // Lógica de SurrealDB para verificar padre (OU) y crear la cuenta
    }
}

// Implementación del puerto para `move_account`
#[async_trait]
impl HierarchyManager for SurrealOrganizationAdapter {
    async fn move_account(&self, ...) -> Result<(), ...> {
        // Lógica transaccional de SurrealDB para mover la cuenta
    }
}

// Implementación del puerto para `get_effective_scps`
#[async_trait]
impl EffectiveScpsProvider for SurrealOrganizationAdapter {
    async fn get_scps_for_resource(&self, resource_hrn: &Hrn) -> Result<HodeiPolicySet, ...> {
        // Lógica compleja del bucle de ascenso
    }
}

// ... y así sucesivamente para todos los traits ...
```

Este plan integral refactoriza `hodei-organizations` para que se alinee perfectamente con la arquitectura final, promoviendo el máximo desacoplamiento y testabilidad, mientras encapsula la lógica compleja de la jerarquía y la herencia de políticas.