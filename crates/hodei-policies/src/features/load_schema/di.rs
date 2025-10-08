//! Dependency Injection for the load_schema feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::ports::SchemaStoragePort;
use super::use_case::LoadSchemaUseCase;
use std::sync::Arc;

/// Factory for creating LoadSchemaUseCase instances
pub struct LoadSchemaUseCaseFactory;

impl LoadSchemaUseCaseFactory {
    /// Creates a new LoadSchemaUseCase instance with injected dependencies
    ///
    /// This factory accepts a storage implementation for retrieving schemas.
    ///
    /// # Arguments
    ///
    /// * `storage` - Implementation of the SchemaStoragePort for retrieving schemas
    ///
    /// # Type Parameters
    ///
    /// * `S` - The concrete storage implementation that must implement SchemaStoragePort
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_policies::features::load_schema::di::LoadSchemaUseCaseFactory;
    /// use hodei_policies::infrastructure::SurrealSchemaStorage;
    ///
    /// let storage = Arc::new(SurrealSchemaStorage::new(db_client));
    /// let use_case = LoadSchemaUseCaseFactory::build(storage);
    ///
    /// let command = LoadSchemaCommand::latest();
    /// let result = use_case.execute(command).await?;
    /// ```
    pub fn build<S: SchemaStoragePort>(storage: Arc<S>) -> LoadSchemaUseCase<S> {
        LoadSchemaUseCase::new(storage)
    }
}
