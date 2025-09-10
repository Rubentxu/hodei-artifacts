// crates/distribution/src/features/generate_npm_metadata/mod.rs

//! Feature para generación de metadatos npm
//! 
//! Esta feature implementa la generación de metadatos de paquetes npm para repositorios,
//! incluyendo la creación de índices de paquetes y metadatos agregados.

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Tests unitarios
#[cfg(test)]
mod dto_test;
#[cfg(test)]
mod use_case_test;
#[cfg(test)]
mod api_test;

// Re-exportar los tipos públicos principales
pub use dto::{
    NpmPackageMetadataDto, GenerateNpmMetadataRequest, GenerateNpmMetadataResponse,
    NpmPackageInfo, NpmPackageVersion, NpmPackageDist, NpmPackageAuthor,
    NpmPackageRepository, NpmPackageKeywords, NpmPackageError,
};
pub use ports::{
    NpmMetadataGenerator, NpmPackageLister, NpmMetadataCache,
    NpmMetadataGeneratorError, NpmPackageListerError, NpmMetadataCacheError,
};
pub use use_case::GenerateNpmMetadataUseCase;
pub use api::GenerateNpmMetadataApi;
pub use di::GenerateNpmMetadataDIContainer;

// Re-exportar tipos de error para conveniencia
pub use dto::NpmPackageError as Error;