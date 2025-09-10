// crates/repository/src/features/delete_repository/mod.rs

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
pub use dto::{DeleteRepositoryCommand, DeleteRepositoryResponse};
pub use api::DeleteRepositoryEndpoint;
pub use di::DeleteRepositoryDIContainer;