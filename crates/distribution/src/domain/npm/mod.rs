// crates/distribution/src/domain/npm/mod.rs

//! Dominio npm - Lógica de negocio pura sin dependencias de infraestructura
//! 
//! Este módulo contiene las entidades, value objects y reglas de negocio
//! específicas del ecosistema npm. Es completamente síncrono y no tiene
//! dependencias de infraestructura.

pub mod package;
pub mod version;
pub mod metadata;
pub mod validation;

// Re-exportar componentes del dominio
pub use package::{NpmPackage, NpmPackageName, NpmPackageValidationError};
pub use version::{NpmVersion, NpmVersionValidationError};
pub use metadata::{NpmPackageMetadata, NpmRepositoryMetadata};
pub use validation::{validate_npm_package_name, validate_npm_version};