//! Feature: Get Effective Policies for Principal
//!
//! Este caso de uso proporciona la ÚNICA forma de que otros crates obtengan
//! las políticas IAM efectivas para un principal.
//!
//! # Contrato Público
//! - Input: `GetEffectivePoliciesQuery` (HRN del principal)
//! - Output: `EffectivePoliciesResponse` (PolicySet de Cedar)
//!
//! # Encapsulación
//! Las entidades internas (User, Group, Policy) NO se exponen.
//! Solo se devuelve un PolicySet de Cedar que puede ser usado directamente
//! por el motor de autorización.

pub mod dto;
pub mod error;
pub mod use_case;

// Re-exports públicos para acceso externo
pub use dto::{EffectivePoliciesResponse, GetEffectivePoliciesQuery};
pub use error::{GetEffectivePoliciesError, GetEffectivePoliciesResult};
pub use use_case::GetEffectivePoliciesForPrincipalUseCase;
