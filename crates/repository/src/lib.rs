// Repository Crate
pub mod domain;
pub mod features;
pub mod infrastructure;

// Re-exportar las features principales
pub use features::create_repository::{
    CreateRepositoryEndpoint, CreateRepositoryDIContainer,
    CreateRepositoryCommand, CreateRepositoryResponse
};

pub use features::get_repository::{
    GetRepositoryEndpoint, GetRepositoryDIContainer,
    GetRepositoryQuery, GetRepositoryResponse
};

pub use features::update_repository::{
    UpdateRepositoryEndpoint, UpdateRepositoryDIContainer,
    UpdateRepositoryCommand, UpdateRepositoryResponse
};

pub use features::delete_repository::{
    DeleteRepositoryEndpoint, DeleteRepositoryDIContainer,
    DeleteRepositoryCommand, DeleteRepositoryResponse
};

// Re-exportar infraestructura
pub use infrastructure::{
    MongoDbRepositoryAdapter,
    UnifiedRepositoryAdapter,
    RepositoryApiModule,
    create_repository_api_module,
    create_repository_api_module_for_testing,
};