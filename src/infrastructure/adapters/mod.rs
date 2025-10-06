//! Módulo de adaptadores de infraestructura
//!
//! Este módulo contiene los adaptadores que conectan los bounded contexts
//! con el kernel compartido y entre sí.

pub mod get_effective_scps_adapter;

pub use get_effective_scps_adapter::GetEffectiveScpsAdapter;

