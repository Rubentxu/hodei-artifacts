// crates/repository/src/domain/mod.rs

pub mod error;
pub mod events;
pub mod policy;
pub mod repository;
pub mod storage;

pub use error::{RepositoryError, RepositoryResult};