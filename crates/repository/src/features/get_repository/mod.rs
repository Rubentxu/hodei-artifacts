// crates/repository/src/features/get_repository/mod.rs

pub mod dto;
pub mod ports;
pub mod use_case;

pub mod api;
pub mod di;

// Tests unitarios
#[cfg(test)]
mod use_case_test;

// Public exports
pub use dto::{GetRepositoryQuery, GetRepositoryResponse};
pub use api::GetRepositoryEndpoint;
pub use di::GetRepositoryDIContainer;