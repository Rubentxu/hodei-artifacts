
// Repository Crate
pub mod domain;
pub mod features;
pub mod infrastructure;

// Re-exportar las features principales
pub use features::create_repository::{
    CreateRepositoryDIContainer as CreateRepositoryFeature,
    CreateRepositoryCommand, CreateRepositoryResponse
};

pub use features::get_repository::{
    GetRepositoryDIContainer as GetRepositoryFeature,
    GetRepositoryQuery, GetRepositoryResponse
};

pub use features::update_repository::{
    UpdateRepositoryDIContainer as UpdateRepositoryFeature,
    UpdateRepositoryCommand, UpdateRepositoryResponse
};

pub use features::delete_repository::{
    DeleteRepositoryDIContainer as DeleteRepositoryFeature,
    DeleteRepositoryCommand, DeleteRepositoryResponse
};
