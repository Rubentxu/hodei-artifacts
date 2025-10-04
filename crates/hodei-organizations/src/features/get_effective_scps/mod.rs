pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Re-exports públicos para acceso externo
pub use dto::{EffectiveScpsResponse, GetEffectiveScpsQuery};
pub use error::GetEffectiveScpsError;
pub use use_case::GetEffectiveScpsUseCase;
