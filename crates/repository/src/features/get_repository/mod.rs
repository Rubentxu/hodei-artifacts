// crates/repository/src/features/get_repository/mod.rs

mod dto;
mod ports;
mod use_case;
mod adapter;
mod api;
mod di;

// Tests unitarios
#[cfg(test)]
mod use_case_test;

// Public exports
pub use dto::{GetRepositoryQuery, GetRepositoryResponse};
pub use api::GetRepositoryEndpoint;
pub use di::GetRepositoryDIContainer;