### Fase 1: Construcción de `hodei-organizations` - La Base de la Gobernanza

Este crate es la fundación. No podemos gobernar si no tenemos una estructura que gobernar.
Stack persistencia SurrealDB Embebida.


#### **HU 1.1: Modelado y Persistencia del `Account`**

*   **Como** un Arquitecto de la Nube,
*   **Quiero** modelar una `Account` como la unidad fundamental de mi organización y poder crearla y recuperarla,
*   **Para que** pueda representar las particiones de recursos y de IAM de mi sistema.

*   **Detalles de Implementación:**
    *   **Dominio (`src/shared/domain/account.rs`):** Crea el struct `Account { hrn: Hrn, name: String, parent_hrn: Hrn }`. El `parent_hrn` apuntará a una OU o a la Raíz.
    *   **Puerto (`.../ports/account_repository.rs`):** Define `trait AccountRepository { async fn save(&self, ...); async fn find_by_hrn(&self, ...); }`.
    *   **Feature (`.../features/create_account/`):** Implementa el `CreateAccountUseCase` que genera un HRN único, crea una instancia de `Account` y la guarda usando el repositorio.

*   **Testing:**
    *   **Unitario:** En `account.rs`, prueba que `Account::new()` inicializa los campos correctamente.
    *   **Integración (`tests/create_account_test.rs`):**
        1.  Setup: Crea una implementación `InMemoryAccountRepository` (usando `Surreal::new::<Mem>()` y un `Mutex<HashMap>`).
        2.  Arrange: Instancia el `CreateAccountUseCase` con el repositorio en memoria.
        3.  Act: Ejecuta el caso de uso con el comando `{ name: "TestAccount" }`.
        4.  Assert: Verifica que el `AccountView` devuelto es correcto. Usa el repositorio para recuperar la `Account` y afirma que sus propiedades coinciden con las esperadas.

---

#### **HU 1.2: Modelado y Persistencia de la `OrganizationalUnit (OU)`**

*   **Como** un Administrador de la Organización,
*   **Quiero** crear y recuperar Unidades Organizativas (OUs),
*   **Para que** pueda empezar a construir la jerarquía de mi organización.

*   **Detalles de Implementación:**
    *   **Dominio (`.../domain/ou.rs`):** Crea el struct `OrganizationalUnit { hrn: Hrn, name: String, parent_hrn: Hrn, child_ous: Vec<Hrn>, child_accounts: Vec<Hrn>, attached_scps: Vec<Hrn> }`.
    *   **Puerto (`.../ports/ou_repository.rs`):** Define el `trait OuRepository`.
    *   **Feature (`.../features/create_ou/`):** Implementa el `CreateOuUseCase`.

*   **Testing:**
    *   **Unitario:** En `ou.rs`, prueba los métodos de negocio como `add_child_account`, `remove_child_account`, etc., para asegurar que manipulan las listas de HRNs correctamente.
    *   **Integración (`tests/create_ou_test.rs`):** Sigue el mismo patrón que para `Account`, usando un `InMemoryOuRepository`.

---

#### **HU 1.3: Mover una `Account` entre OUs**

*   **Como** un Administrador de la Organización,
*   **Quiero** mover una `Account` existente de una OU a otra,
*   **Para que** pueda reestructurar mi organización a medida que evoluciona.

*   **Detalles de Implementación:**
    *   **Feature (`.../features/move_account/`):** El `MoveAccountUseCase` es el primer caso de uso que coordina entre múltiples agregados.
        *   Necesitará ser inyectado con `Arc<dyn AccountRepository>` y `Arc<dyn OuRepository>`.
        *   Su lógica debe ser:
            1.  Cargar la `Account` a mover.
            2.  Cargar la `OU` de origen y la `OU` de destino.
            3.  Llamar a `ou_origen.remove_child_account(...)`.
            4.  Llamar a `account.set_parent(...)`.
            5.  Llamar a `ou_destino.add_child_account(...)`.
            6.  Guardar los tres agregados modificados (`account`, `ou_origen`, `ou_destino`).

*   **Testing:**
    *   **Integración (`tests/move_account_test.rs`):** Este test es crucial.
        1.  Setup: Crea repos en memoria para OUs y Accounts.
        2.  Arrange: Puebla los repos con una `Account` "WebApp", una `OU` "Staging" y una `OU` "Production". La cuenta "WebApp" debe estar inicialmente en "Staging".
        3.  Act: Ejecuta el `MoveAccountUseCase` para mover "WebApp" a "Production".
        4.  Assert:
            *   Recupera la `Account` "WebApp" y afirma que su `parent_hrn` ahora apunta a "Production".
            *   Recupera la `OU` "Staging" y afirma que su lista `child_accounts` está vacía.
            *   Recupera la `OU` "Production" y afirma que su lista `child_accounts` ahora contiene el HRN de "WebApp".

---

#### **HU 1.4: Gestión Básica de `ServiceControlPolicy (SCP)`**

*   **Como** un Administrador de Gobernanza,
*   **Quiero** crear y adjuntar una SCP a una OU,
*   **Para que** pueda definir una barrera de permisos para todas las cuentas dentro de esa OU.

*   **Detalles de Implementación:**
    *   **Dominio (`.../domain/scp.rs`):** Define `ServiceControlPolicy { hrn: Hrn, name: String, document: String }`.
    *   **Puerto (`.../ports/scp_repository.rs`):** Define `trait ScpRepository`.
    *   **Feature (`.../features/create_scp/`):** Implementa el caso de uso para crear la SCP.
    *   **Feature (`.../features/attach_scp/`):** Implementa el caso de uso para adjuntarla. Este caso de uso cargará la OU (o Account) y la SCP, llamará al método de dominio `ou.attach_scp(...)` y guardará la OU actualizada.

*   **Testing:**
    *   **Integración (`tests/attach_scp_test.rs`):**
        1.  Arrange: Crea una `OU` y una `SCP` en los repos en memoria.
        2.  Act: Ejecuta el `AttachScpUseCase`.
        3.  Assert: Recupera la `OU` y afirma que su `attached_scps` ahora contiene el HRN de la `SCP`.

---

### Fase 2: Construcción de `hodei-authorizer` - El Cerebro Orquestador

Este crate no tiene dominio propio, es pura lógica de aplicación y orquestación.

#### **HU 2.1: Definición de Contratos y Mocks**

*   **Como el** desarrollador del `Authorizer`,
*   **Necesito** definir los traits (`...Provider`) que describen los datos que necesito de `hodei-iam` y `hodei-organizations`,
*   **Para que** pueda desarrollar la lógica de decisión de forma aislada y testable.

*   **Detalles de Implementación:**
    *   **Crate `hodei-authorizer` (`src/ports.rs`):**
        *   Define `trait IamPolicyProvider { async fn get_identity_policies_for(&self, ...) -> ...; }`.
        *   Define `trait OrganizationBoundaryProvider { async fn get_effective_scps_for(&self, ...) -> ...; }`.
    *   **Mocks (`src/tests/mocks.rs`):** Crea structs `MockIamPolicyProvider` y `MockOrgBoundaryProvider` que implementen estos traits y te permitan configurar qué datos devuelven en los tests.

*   **Testing:**
    *   **Unitario:** Escribe tests para tus mocks para asegurar que devuelven los datos con los que los configuras.

---

#### **HU 2.2: Implementación de la Regla "Deny Explícito Anula Todo"**

*   **Como el** `AuthorizerService`,
*   **Quiero** recolectar TODAS las políticas aplicables (IAM y SCPs) y si CUALQUIERA de ellas contiene un `Deny` explícito para la petición, la decisión final debe ser `Deny` inmediatamente,
*   **Para que** se cumpla la regla de seguridad más fundamental.

*   **Detalles de Implementación:**
    *   En `AuthorizerService::is_authorized`:
        1.  Llama a `iam_provider.get_identity_policies_for(...)`.
        2.  Llama a `org_provider.get_effective_scps_for(...)`.
        3.  Combina ambos conjuntos de políticas en un único `PolicySet`.
        4.  Usa el `PolicyEvaluator` de `hodei-policies` para evaluar este `PolicySet`.
        5.  Si `response.decision() == Decision::Deny`, `return response;`.

*   **Testing:**
    *   **Integración (`tests/deny_rule_test.rs`):**
        1.  Setup: Instancia el `AuthorizerService` con tus proveedores mock.
        2.  Arrange: Configura el `MockOrgBoundaryProvider` para que devuelva una SCP con `forbid(principal, action, resource);`. Configura el `MockIamPolicyProvider` para que devuelva una política con `permit(...)`.
        3.  Act: Llama a `authorizer.is_authorized(...)`.
        4.  Assert: Afirma que la decisión final es `Deny`.

---

#### **HU 2.3: Implementación de la Regla "Se Requiere un Allow de Identidad"**

*   **Como el** `AuthorizerService`,
*   **Quiero**, si no hubo un `Deny` explícito, evaluar únicamente las políticas de IAM,
*   **Para que** pueda determinar si la identidad del principal tiene permiso para realizar la acción.

*   **Detalles de Implementación:**
    *   Añade el siguiente bloque de lógica a `is_authorized` después del chequeo de `Deny`.
        1.  Toma *solo* las políticas del `IamPolicyProvider`.
        2.  Evalúalas con el `PolicyEvaluator`.
        3.  Si la decisión no es `Allow`, la decisión final es `Deny` (implícito).

*   **Testing:**
    *   **Integración (`tests/iam_allow_rule_test.rs`):**
        1.  Arrange: Configura los mocks para que no devuelvan ninguna política de `Deny`. Configura el `MockIamPolicyProvider` para que devuelva una política `permit(...)` que coincida con la petición.
        2.  Act: Llama a `authorizer.is_authorized(...)`.
        3.  Assert: Afirma que la decisión final es `Allow` (por ahora, antes de la última regla).

---

#### **HU 2.4: Implementación de la Regla "Las Barreras de la Organización Deben Permitir"**

*   **Como el** `AuthorizerService`,
*   **Quiero**, si una acción está permitida por IAM, verificar adicionalmente que también está permitida por las barreras de las SCPs,
*   **Para que** las políticas de gobernanza actúen como un filtro final sobre los permisos concedidos.

*   **Detalles de Implementación:**
    *   Añade el último bloque de lógica.
        1.  Si el chequeo de IAM dio `Allow`, ahora toma *solo* las políticas del `OrganizationBoundaryProvider`.
        2.  Evalúalas.
        3.  Si la decisión de esta evaluación *no* es `Allow`, la decisión final se convierte en `Deny`. Si es `Allow`, la decisión final se mantiene como `Allow`.

*   **Testing:**
    *   **Integración (`tests/scp_boundary_rule_test.rs`):**
        1.  Arrange: Configura `MockIamPolicyProvider` para que devuelva una política `permit(...)` para `action::"s3:GetObject"`. Configura `MockOrgBoundaryProvider` para que devuelva una SCP que *no* menciona `s3:GetObject` (por ejemplo, solo permite `ec2:*`).
        2.  Act: Llama a `authorizer.is_authorized(...)` pidiendo `s3:GetObject`.
        3.  Assert: Afirma que la decisión final es `Deny`, porque aunque IAM lo permitió, la barrera de la SCP no lo hizo (Deny implícito de la barrera).

---


# Feature Specification: Governance & Authorization Core

**Feature Branch**: `feat/governance-auth-core`
**Created**: 2025-10-02
**Status**: Draft
**Input**: User description: "Implementar un sistema de gobernanza tipo AWS Organizations con SCPs y un orquestador de autorización central que combine las políticas de gobernanza con las políticas de IAM para tomar decisiones de acceso seguras y jerárquicas."

## User Scenarios & Testing

### Primary User Story
Como Arquitecto de Seguridad, quiero definir barreras de permisos a nivel de organización (SCPs) que restrinjan lo que los administradores de cuentas individuales pueden hacer, para garantizar que se cumplan las políticas de gobernanza corporativa, incluso si se conceden permisos excesivos a nivel de IAM.

### Acceptance Scenarios
1.  **Given** una OU "Production" tiene una SCP adjunta que explícitamente **deniega** la acción `iam:DeleteUser`,
    **And** una Cuenta "WebApp" está dentro de la OU "Production",
    **And** un `User` "Admin" dentro de la cuenta "WebApp" tiene una política de IAM que **permite** `iam:*` (todos los permisos),
    **When** el "Admin" intenta realizar la acción `iam:DeleteUser`,
    **Then** la petición es **denegada**.

2.  **Given** una OU "Sandbox" tiene una SCP adjunta que **permite** la acción `s3:GetObject` y `ec2:*`,
    **And** una Cuenta "DevAccount" está dentro de la OU "Sandbox",
    **And** un `User` "Developer" dentro de "DevAccount" tiene una política de IAM que **permite** `s3:GetObject`,
    **When** el "Developer" intenta realizar la acción `s3:GetObject`,
    **Then** la petición es **permitida**.

3.  **Given** una OU "Sandbox" tiene una SCP adjunta que **permite** solo `ec2:*`,
    **And** una Cuenta "DevAccount" está dentro de la OU "Sandbox",
    **And** un `User` "Developer" dentro de "DevAccount" tiene una política de IAM que **permite** `s3:GetObject`,
    **When** el "Developer" intenta realizar la acción `s3:GetObject`,
    **Then** la petición es **denegada** (porque la barrera de la SCP no lo permite).

### Edge Cases
- ¿Qué sucede si una entidad tiene múltiples SCPs heredadas (de la Raíz, de OUs anidadas)? El sistema debe evaluar la unión de todas las SCPs aplicables.
- ¿Cómo maneja el sistema una Cuenta que no está en ninguna OU (directamente bajo la Raíz)? Debe heredar las SCPs de la Raíz.

## Requirements

### Functional Requirements
- **FR-001**: El sistema DEBE permitir la creación de una jerarquía de Unidades Organizativas (OUs) y Cuentas.
- **FR-002**: El sistema DEBE permitir la creación de Políticas de Control de Servicio (SCPs) que contengan documentos de políticas de Cedar.
- **FR-003**: El sistema DEBE permitir adjuntar y desadjuntar SCPs a la Raíz, OUs o Cuentas.
- **FR-004**: El sistema DEBE proveer un servicio de autorización central (`hodei-authorizer`).
- **FR-005**: El servicio de autorización DEBE denegar una acción si CUALQUIER política aplicable (IAM o SCP) contiene un `forbid` explícito que coincida.
- **FR-006**: Si no hay un `forbid` explícito, el servicio de autorización DEBE requerir que un `permit` explícito exista en las políticas de IAM.
- **FR-007**: Si un `permit` de IAM existe, el servicio de autorización DEBE verificar adicionalmente que la acción está implícita o explícitamente permitida por la unión de todas las SCPs efectivas.

### Key Entities
- **Organization**: La entidad raíz que contiene todo.
- **OrganizationalUnit (OU)**: Un contenedor para otras OUs o Cuentas.
- **Account**: Una partición que contiene recursos y principales de IAM.
- **ServiceControlPolicy (SCP)**: Una política de gobernanza que define barreras.

---
---

# Implementation Plan: Governance & Authorization Core

**Input**: Design documents from `specs/governance-auth-core/`
**Prerequisites**: `hodei-iam` y `hodei-policies` (refactorizado) existen.

## Phase 3.1: Setup
- [ ] T001 [P] Crear la estructura del crate `hodei-organizations` en `crates/hodei-organizations/`
- [ ] T002 [P] Crear la estructura del crate `hodei-authorizer` en `crates/hodei-authorizer/`
- [ ] T003 [P] Añadir los nuevos crates al `Cargo.toml` del workspace.
- [ ] T004 [P] Configurar dependencias: `hodei-organizations` depende de `hodei-policies`. `hodei-authorizer` depende de los tres.

## Phase 3.2: Tests First (TDD) ⚠️ MUST COMPLETE BEFORE 3.3

### `hodei-organizations`
- [ ] T005 [P] Integration test para `CreateAccountUseCase` en `crates/hodei-organizations/tests/create_account_test.rs`
- [ ] T006 [P] Integration test para `CreateOuUseCase` en `crates/hodei-organizations/tests/create_ou_test.rs`
- [ ] T007 [P] Integration test para `MoveAccountUseCase` en `crates/hodei-organizations/tests/move_account_test.rs`
- [ ] T008 [P] Integration test para `AttachScpUseCase` en `crates/hodei-organizations/tests/attach_scp_test.rs`
- [ ] T009 [P] Integration test para `GetEffectiveScpsUseCase` en `crates/hodei-organizations/tests/get_effective_scps_test.rs` (Debe simular una jerarquía y verificar que se recolectan las SCPs correctas)

### `hodei-authorizer`
- [ ] T010 [P] Integration test para la regla "Deny explícito de SCP anula Allow de IAM" en `crates/hodei-authorizer/tests/deny_scp_overrides_iam_test.rs`
- [ ] T011 [P] Integration test para la regla "Deny explícito de IAM anula todo" en `crates/hodei-authorizer/tests/deny_iam_overrides_all_test.rs`
- [ ] T012 [P] Integration test para la regla "Se requiere Allow de IAM y Allow de SCP" en `crates/hodei-authorizer/tests/allow_requires_both_test.rs`

## Phase 3.3: Core Implementation (ONLY after tests are failing)

### `hodei-organizations`
- [ ] T013 [P] Modelo de dominio `Account` en `crates/hodei-organizations/src/shared/domain/account.rs`
- [ ] T014 [P] Modelo de dominio `OrganizationalUnit` en `crates/hodei-organizations/src/shared/domain/ou.rs`
- [ ] T015 [P] Modelo de dominio `ServiceControlPolicy` en `crates/hodei-organizations/src/shared/domain/scp.rs`
- [ ] T016 [P] Puertos de Repositorio para `Account`, `OU`, `SCP` en `crates/hodei-organizations/src/shared/application/ports/`
- [ ] T017 Feature `CreateAccountUseCase` en `crates/hodei-organizations/src/features/create_account/`
- [ ] T018 Feature `CreateOuUseCase` en `crates/hodei-organizations/src/features/create_ou/`
- [ ] T019 Feature `MoveAccountUseCase` en `crates/hodei-organizations/src/features/move_account/`
- [ ] T020 Feature `AttachScpUseCase` en `crates/hodei-organizations/src/features/attach_scp/`
- [ ] T021 Feature `GetEffectiveScpsUseCase` en `crates/hodei-organizations/src/features/get_effective_scps/`

### `hodei-authorizer`
- [ ] T022 [P] Definir traits `IamPolicyProvider` y `OrganizationBoundaryProvider` en `crates/hodei-authorizer/src/ports.rs`
- [ ] T023 Implementar la lógica de decisión en `AuthorizerService` en `crates/hodei-authorizer/src/authorizer.rs`

## Phase 3.4: Integration
- [ ] T024 [P] Implementar adaptadores de repositorio para `SurrealDB` para `Account`, `OU`, `SCP` en `crates/hodei-organizations/src/shared/infrastructure/surreal/`
- [ ] T025 Implementar el adaptador `OrganizationBoundaryProvider` en `hodei-organizations` que use el `GetEffectiveScpsUseCase`.
- [ ] T026 Implementar el adaptador `IamPolicyProvider` en `hodei-iam`.
- [ ] T027 Refactorizar `hodei-policies` para ser un motor puro (`PolicyEvaluator`).

## Phase 3.5: Polish
- [ ] T028 [P] Unit tests para la lógica de dominio de `OU` (ej. `add_child`) en `crates/hodei-organizations/src/shared/domain/ou.rs`
- [ ] T029 [P] Documentación de la API pública para `hodei-authorizer`.
- [ ] T030 Crear un test E2E completo en `tests/` del workspace que configure los 3 crates y verifique un escenario complejo.

## Dependencies
- T005-T012 (Tests) deben estar escritos y fallando antes de T013-T023.
- T013, T014, T015 (Modelos) bloquean sus respectivos repositorios y casos de uso.
- T022 (Puertos de Authorizer) bloquea T023 (Lógica de Authorizer).
- T023 bloquea T025 y T026 (Adaptadores).
- T027 (Refactor de Policies) es un prerrequisito para T023.

## Parallel Example
```
# Fase de Tests (TDD). Pueden escribirse todos en paralelo.
Task: "Integration test para `CreateAccountUseCase` en crates/hodei-organizations/tests/create_account_test.rs"
Task: "Integration test para `CreateOuUseCase` en crates/hodei-organizations/tests/create_ou_test.rs"
Task: "Integration test para la regla 'Deny explícito de SCP anula Allow de IAM' en crates/hodei-authorizer/tests/deny_scp_overrides_iam_test.rs"
...
```

## Notes
- Cada `feature` en `hodei-organizations` debe tener su propia "vertical slice" (`dto`, `use_case`, `di`).
- Los tests de integración son la clave. Usarán implementaciones de repositorios en memoria (`Surreal::new::<Mem>()`) para aislar el test de una base de datos real.
- El `PolicyEvaluator` en `hodei-policies` será un componente simple y sin estado, fácil de instanciar y usar en los tests de `hodei-authorizer`.