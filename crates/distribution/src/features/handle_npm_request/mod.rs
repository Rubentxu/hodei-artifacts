// crates/distribution/src/features/handle_npm_request/mod.rs

//! Feature Handle NPM Request - Manejo de solicitudes npm con arquitectura VSA
//! 
//! Este feature implementa el manejo completo de paquetes npm siguiendo la 
//! Vertical Slice Architecture (VSA) con Clean Architecture principles.
//! 
//! # Características principales
//! - Soporte para paquetes npm con y sin scope
//! - Gestión de dist-tags (latest, beta, etc.)
//! - Validación de nombres y versiones npm
//! - Almacenamiento en S3 con estructura npm
//! - Integración con MongoDB para metadatos
//! - Control de permisos con Cedar
//! - Logging estructurado con tracing

// Capa de dominio - Modelos y lógica pura
pub mod dto;
pub mod ports;

// Capa de aplicación - Casos de uso
pub mod use_case;

// Capa de infraestructura - Adaptadores concretos
pub mod adapter;

// Capa de presentación - API y endpoints
pub mod api;

// Configuración de dependencias
pub mod di;

// Re-exportar los tipos principales para facilitar el uso
pub use dto::{
    GetPackageRequest, GetPackageResponse, PutPackageRequest, PutPackageResponse,
    HeadPackageRequest, HeadPackageResponse, GetPackageJsonRequest, GetPackageJsonResponse,
    GetRepositoryInfoRequest, GetRepositoryInfoResponse, SearchRequest, SearchResponse,
    GetDistTagsRequest, GetDistTagsResponse, UpdateDistTagsRequest, UpdateDistTagsResponse,
    NpmPackageDto, NpmPackageJsonDto, NpmRepositoryInfoDto, NpmSearchResultDto,
    NpmDistTagsDto,
};

pub use ports::{
    NpmPackageReader, NpmPackageWriter, NpmRepositoryManager, NpmPermissionChecker,
    NpmPackageReaderError, NpmPackageWriterError, NpmRepositoryManagerError, NpmPermissionCheckerError,
};

pub use use_case::{
    HandleNpmGetPackageUseCase, HandleNpmPutPackageUseCase, HandleNpmHeadPackageUseCase,
    HandleNpmGetPackageJsonUseCase, HandleNpmGetRepositoryInfoUseCase, HandleNpmSearchUseCase,
    HandleNpmGetDistTagsUseCase, HandleNpmUpdateDistTagsUseCase,
};

pub use api::NpmRequestHandler;

pub use di::HandleNpmRequestDIContainer;

// Re-exportar tipos del dominio npm
pub use crate::domain::npm::{
    NpmPackageName, NpmVersion, NpmVersionRange, NpmPackageMetadata, NpmPackageJson,
    validate_npm_package_name, validate_npm_version, validate_npm_version_range,
};