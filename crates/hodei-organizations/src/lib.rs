//! hodei-organizations: Organization structure and Service Control Policies
//!
//! This crate manages organizational units, accounts, and SCPs.
//! External access is ONLY through features (use cases).
//!
//! # Architecture
//! - Internal: domain entities, repositories, adapters
//! - External: ONLY features/use cases with Commands/Queries/DTOs

pub mod features;
pub mod shared;

// ❌ NO exportar entidades de dominio - son INTERNAS
// Solo se accede a este crate a través de sus casos de uso (features)

// ✅ Re-export features/casos de uso para acceso externo
pub use features::{
    attach_scp::use_case::AttachScpUseCase,
    create_account::use_case::CreateAccountUseCase,
    create_ou::use_case::CreateOuUseCase,
    get_effective_scps::{
        dto::{EffectiveScpsResponse, GetEffectiveScpsQuery},
        use_case::GetEffectiveScpsUseCase,
    },
};
