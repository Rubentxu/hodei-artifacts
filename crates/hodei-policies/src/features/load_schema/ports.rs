//! Ports (trait definitions) for the load_schema feature
//!
//! This module re-exports the SchemaStoragePort from build_schema
//! since both features share the same storage abstraction.
//!
//! Following the DRY principle and ISP (Interface Segregation Principle),
//! we reuse the existing storage port rather than defining a duplicate.

use crate::features::load_schema::dto::{LoadSchemaCommand, LoadSchemaResult};
use crate::features::load_schema::error::LoadSchemaError;
use async_trait::async_trait;

// Re-export the SchemaStoragePort from build_schema
pub use crate::features::build_schema::ports::SchemaStoragePort;

/// Port trait for loading Cedar schemas from storage
///
/// This trait defines the contract for schema loading operations.
/// It represents the use case's public interface.
#[async_trait]
pub trait LoadSchemaPort: Send + Sync {
    /// Load a Cedar schema from storage
    ///
    /// This method retrieves a schema from storage, either by version
    /// or loading the latest available schema.
    ///
    /// # Arguments
    ///
    /// * `command` - Configuration specifying which schema to load
    ///
    /// # Returns
    ///
    /// The loaded schema along with its version and identifier
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The requested schema version doesn't exist
    /// - No schemas are available (when loading latest)
    /// - The storage backend is unavailable
    /// - Schema parsing fails
    async fn execute(
        &self,
        command: LoadSchemaCommand,
    ) -> Result<LoadSchemaResult, LoadSchemaError>;
}
