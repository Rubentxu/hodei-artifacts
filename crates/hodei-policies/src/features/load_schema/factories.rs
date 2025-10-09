//! Factory functions for the load_schema feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::load_schema::ports::{LoadSchemaPort, SchemaStoragePort};
use crate::features::load_schema::use_case::LoadSchemaUseCase;
use std::sync::Arc;

/// Creates a LoadSchemaUseCase with the provided dependencies
///
/// This factory receives an already-constructed schema storage implementation
/// and assembles a use case for loading schemas.
///
/// # Arguments
///
/// * `storage` - Pre-constructed implementation of SchemaStoragePort
///
/// # Returns
///
/// An `Arc<dyn LoadSchemaPort>` trait object, enabling dependency inversion
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::load_schema::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapter
/// let schema_storage = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Factory receives the adapter and assembles the use case
/// let use_case = factories::create_load_schema_use_case(schema_storage);
/// let result = use_case.execute(command).await?;
/// ```
pub fn create_load_schema_use_case<S: SchemaStoragePort + 'static>(
    storage: Arc<S>,
) -> Arc<dyn LoadSchemaPort> {
    Arc::new(LoadSchemaUseCase::new(storage))
}
