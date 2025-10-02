// Exportaciones públicas de la feature
pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

// Solo exponer lo necesario al exterior
pub use api::ValidatePolicyApi;
pub use di::ValidatePolicyDIContainer;
pub use dto::{ValidatePolicyRequest, ValidatePolicyResponse};
pub use error::ValidatePolicyError;
