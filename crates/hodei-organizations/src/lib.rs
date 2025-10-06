//! # hodei-organizations - Organizations Bounded Context
//!
//! Este crate gestiona la estructura organizacional de Hodei, incluyendo:
//! - Cuentas (Accounts)
//! - Unidades Organizacionales (OUs)
//! - Políticas de Control de Servicios (SCPs)
//!
//! ## Arquitectura
//!
//! Este bounded context sigue los principios de Clean Architecture y Vertical Slice Architecture (VSA):
//! - **Encapsulamiento estricto**: Solo se expone la API pública a través de casos de uso
//! - **Features independientes**: Cada feature tiene sus propios ports, adapters y DTOs
//! - **Dominio privado**: Las entidades de dominio son detalles de implementación internos
//!
//! ## API Pública
//!
//! ### Casos de Uso (Use Cases)
//!
//! ```rust,ignore
//! use hodei_organizations::{
//!     CreateAccountUseCase,
//!     CreateOuUseCase,
//!     CreateScpUseCase,
//!     AttachScpUseCase,
//!     GetEffectiveScpsUseCase,
//!     MoveAccountUseCase,
//! };
//! ```
//!
//! ### Eventos de Dominio
//!
//! Los eventos de dominio son públicos para permitir la integración con otros bounded contexts:
//!
//! ```rust,ignore
//! use hodei_organizations::events::{AccountCreated, ScpAttached};
//! ```
//!
//! ### Adaptador Cross-Context
//!
//! Para integración con el kernel compartido:
//!
//! ```rust,ignore
//! use hodei_organizations::GetEffectiveScpsAdapter;
//! use kernel::GetEffectiveScpsPort;
//! ```
//!
//! ## Ejemplo de Uso
//!
//! ```rust,ignore
//! use hodei_organizations::{CreateAccountUseCase, CreateAccountCommand};
//!
//! async fn create_account(use_case: &CreateAccountUseCase<...>) {
//!     let command = CreateAccountCommand {
//!         account_name: "production".to_string(),
//!         parent_ou_hrn: "hrn:hodei:organizations::ou/root".to_string(),
//!     };
//!
//!     let result = use_case.execute(command).await?;
//!     println!("Created account: {}", result.account_hrn);
//!     Ok(())
//! }
//! ```

// ============================================================================
// Public API - Features (Use Cases)
// ============================================================================

pub mod features;

// ============================================================================
// Internal Modules (Private)
// ============================================================================

/// Módulo interno con dominio, puertos y adaptadores.
/// NO exportar públicamente - son detalles de implementación.
mod internal;

// ============================================================================
// Public Exports - Use Cases
// ============================================================================

/// Feature: Crear una nueva cuenta
pub use features::create_account::{
    dto::{AccountView, CreateAccountCommand},
    error::CreateAccountError,
    use_case::CreateAccountUseCase,
};

/// Feature: Crear una nueva unidad organizacional (OU)
pub use features::create_ou::{
    dto::{CreateOuCommand, OuView},
    error::CreateOuError,
    use_case::CreateOuUseCase,
};

/// Feature: Crear una nueva política de control de servicios (SCP)
pub use features::create_scp::{
    dto::{CreateScpCommand, ScpDto},
    error::CreateScpError,
    use_case::CreateScpUseCase,
};

/// Feature: Adjuntar una SCP a una cuenta o OU
pub use features::attach_scp::{
    dto::{AttachScpCommand, AttachScpView},
    error::AttachScpError,
    use_case::AttachScpUseCase,
};

/// Feature: Obtener las SCPs efectivas para un recurso
pub use features::get_effective_scps::{
    dto::{EffectiveScpsResponse, GetEffectiveScpsQuery},
    error::GetEffectiveScpsError,
    use_case::GetEffectiveScpsUseCase,
};

/// Feature: Mover una cuenta a una nueva OU
pub use features::move_account::{
    dto::{AccountView as MoveAccountView, MoveAccountCommand},
    error::MoveAccountError,
    use_case::MoveAccountUseCase,
};

// ============================================================================
// Public Exports - Domain Events
// ============================================================================

/// Eventos de dominio emitidos por este bounded context.
/// Públicos para permitir suscripción desde otros contextos.
pub mod events {
    pub use crate::internal::domain::events::{
        AccountCreated, AccountDeleted, AccountMoved, OrganizationalUnitCreated,
        OrganizationalUnitDeleted, ScpAttached, ScpCreated, ScpDeleted, ScpDetached, ScpUpdated,
    };
}

// ============================================================================
// Cross-Context Adapter
// ============================================================================

/// Adaptador que implementa el puerto transversal `GetEffectiveScpsPort` del kernel,
/// exponiendo el caso de uso interno `GetEffectiveScpsUseCase` de manera desacoplada.
///
/// Este adaptador permite a otros bounded contexts (como el autorizador) obtener
/// las SCPs efectivas sin acoplarse a los detalles internos de este contexto.
///
/// # Ejemplo
///
/// ```rust,ignore
/// use hodei_organizations::{GetEffectiveScpsAdapter, GetEffectiveScpsUseCase};
/// use kernel::GetEffectiveScpsPort;
///
/// let use_case = GetEffectiveScpsUseCase::new(scp_repo, org_repo);
/// let adapter = GetEffectiveScpsAdapter::new(use_case);
///
/// // El adaptador implementa GetEffectiveScpsPort
/// let result = adapter.get_effective_scps(query).await?;
/// ```
pub struct GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: features::get_effective_scps::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: features::get_effective_scps::ports::OuRepositoryPort
        + features::get_effective_scps::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    inner: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>,
}

impl<ScpRepo, OrgRepo> GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: features::get_effective_scps::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: features::get_effective_scps::ports::OuRepositoryPort
        + features::get_effective_scps::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    /// Crea un nuevo adaptador wrapeando el caso de uso interno
    pub fn new(inner: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>) -> Self {
        Self { inner }
    }
}

#[::async_trait::async_trait]
impl<ScpRepo, OrgRepo> ::kernel::GetEffectiveScpsPort for GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: features::get_effective_scps::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: features::get_effective_scps::ports::OuRepositoryPort
        + features::get_effective_scps::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    async fn get_effective_scps(
        &self,
        query: ::kernel::GetEffectiveScpsQuery,
    ) -> Result<::cedar_policy::PolicySet, Box<dyn std::error::Error + Send + Sync>> {
        // Convertir el DTO transversal al DTO interno del caso de uso
        let internal_query = GetEffectiveScpsQuery {
            resource_hrn: query.resource_hrn,
        };

        // Ejecutar caso de uso interno
        let response = self
            .inner
            .execute(internal_query)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        // Retornar el PolicySet
        Ok(response.policies)
    }
}

/// Alias ergonómico para inyección dinámica del puerto transversal
pub type DynGetEffectiveScpsPort = std::sync::Arc<dyn ::kernel::GetEffectiveScpsPort>;

// ============================================================================
// DEPRECACIONES TEMPORALES (Para migración - Eliminar en Phase 2)
// ============================================================================

/// ⚠️ DEPRECATED: No usar directamente la infraestructura.
/// Use los casos de uso públicos en su lugar.
#[deprecated(
    since = "0.2.0",
    note = "Direct infrastructure access will be removed. Use public use cases instead."
)]
pub mod __internal_infra_only {
    pub use crate::internal::infrastructure::*;
}

/// ⚠️ DEPRECATED: No usar directamente los puertos genéricos.
/// Cada feature define sus propios puertos segregados.
#[deprecated(
    since = "0.2.0",
    note = "Generic repository ports will be removed. Use feature-specific ports instead."
)]
pub mod __internal_ports_only {
    pub use crate::internal::application::ports::*;
}

// ============================================================================
// Re-exports para DI (Temporal - Eliminar cuando DI esté en capa de aplicación)
// ============================================================================

/// ⚠️ INTERNO: Solo para configuración de DI en la capa de aplicación.
/// No usar directamente en código de negocio.
#[doc(hidden)]
pub mod __internal_di_only {
    pub use crate::internal::infrastructure::surreal::{
        SurrealUnitOfWork, SurrealUnitOfWorkFactory,
    };
}
