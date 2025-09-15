// crates/artifact/src/features/versioning/error.rs

use thiserror::Error;

/// Errores específicos relacionados con el versionado de artefactos
#[derive(Debug, Error)]
pub enum VersioningError {
    #[error("Invalid semantic version: {0}")]
    InvalidSemVer(String),

    #[error("Version does not follow SemVer 2.0.0 specification: {0}")]
    NotSemVerCompliant(String),

    #[error("Snapshot version policy violation: {0}")]
    SnapshotPolicyViolation(String),

    #[error("Pre-release tag not allowed: {0}")]
    PrereleaseTagNotAllowed(String),

    #[error("Build metadata not allowed: {0}")]
    BuildMetadataNotAllowed(String),

    #[error("Version already exists: {0}")]
    VersionAlreadyExists(String),

    #[error("Repository configuration error: {0}")]
    RepositoryConfigError(String),
}
