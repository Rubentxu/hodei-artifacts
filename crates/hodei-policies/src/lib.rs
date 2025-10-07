//! # Hodei Policies Crate
//!
//! This crate provides pure policy evaluation and validation services using Cedar.
//! It acts as the single bounded context with knowledge of the Cedar authorization engine.
//!
//! ## Architecture
//!
//! - **Features**: Vertical slices for specific functionalities (validate_policy, evaluate_policies)
//! - **API**: Centralized public interface
//! - **Internal**: Implementation details (translator, schema_builder)

pub mod features;
pub(crate) mod internal;

// API pública
pub mod api;
pub use api::*;
