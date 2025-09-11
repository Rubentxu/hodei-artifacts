// crates/repository/src/infrastructure/mod.rs

pub mod mongodb_adapter;
pub mod unified_adapter;
pub mod api;

pub use mongodb_adapter::MongoDbRepositoryAdapter;
pub use unified_adapter::{UnifiedRepositoryAdapter, UnifiedRepositoryAdapterBuilder, create_unified_di_container, EventPublisherAdapter};
pub use api::{RepositoryApiModule, RepositoryApiModuleBuilder, create_repository_api_module};

#[cfg(test)]
pub use api::create_repository_api_module_for_testing;