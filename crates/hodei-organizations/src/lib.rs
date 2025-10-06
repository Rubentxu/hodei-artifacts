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
// Public Exports - Ports
// ============================================================================

/// Re-exportación de los puertos públicos de cada feature.
/// Estos traits definen los contratos que los adaptadores deben cumplir.
pub mod ports {
    /// Puertos para la feature get_effective_scps
    pub use crate::features::get_effective_scps::ports::{
        AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
    };

    /// Puertos para features transaccionales
    pub use crate::features::create_account::ports::{
        CreateAccountUnitOfWork, CreateAccountUnitOfWorkFactory,
    };
    pub use crate::features::create_ou::ports::{
        CreateOuUnitOfWork, CreateOuUnitOfWorkFactory,
    };
    pub use crate::features::move_account::ports::{
        MoveAccountUnitOfWork, MoveAccountUnitOfWorkFactory,
    };
}

// ============================================================================
// Public Exports - Infrastructure Adapters (for Composition Root)
// ============================================================================

/// Adaptadores de infraestructura para SurrealDB.
///
/// Estos adaptadores implementan los puertos de cada feature usando SurrealDB
/// con transacciones REALES. Solo deben ser usados en la composition root
/// de la aplicación (main.rs o módulo de configuración).
///
/// # Ejemplo de Uso
///
/// ```rust,no_run
/// use hodei_organizations::infrastructure::{
///     CreateAccountSurrealUnitOfWorkFactoryAdapter,
///     CreateOuSurrealUnitOfWorkFactoryAdapter,
/// };
/// use hodei_organizations::CreateAccountUseCase;
/// use hodei_organizations::__internal_di_only::SurrealUnitOfWorkFactory;
/// use surrealdb::{Surreal, engine::remote::ws::Ws};
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // En la composition root (main.rs)
/// let db = Surreal::new::<Ws>("ws://localhost:8000").await?;
/// db.use_ns("hodei").use_db("production").await?;
///
/// let surreal_factory = Arc::new(SurrealUnitOfWorkFactory::new(Arc::new(db)));
///
/// // Crear adaptadores para cada feature
/// let create_account_factory = CreateAccountSurrealUnitOfWorkFactoryAdapter::new(
///     surreal_factory.clone()
/// );
///
/// // Inyectar en los casos de uso
/// let create_account_use_case = CreateAccountUseCase::new(
///     Arc::new(create_account_factory),
///     "aws".to_string(),
///     "123456789012".to_string(),
/// );
/// # Ok(())
/// # }
/// ```
pub mod infrastructure {
    pub use crate::features::create_account::surreal_adapter::{
        CreateAccountSurrealUnitOfWorkAdapter,
        CreateAccountSurrealUnitOfWorkFactoryAdapter,
    };
    pub use crate::features::create_ou::surreal_adapter::{
        CreateOuSurrealUnitOfWorkAdapter,
        CreateOuSurrealUnitOfWorkFactoryAdapter,
    };
    pub use crate::features::move_account::surreal_adapter::{
        MoveAccountSurrealUnitOfWorkAdapter,
        MoveAccountSurrealUnitOfWorkFactoryAdapter,
    };
}

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
