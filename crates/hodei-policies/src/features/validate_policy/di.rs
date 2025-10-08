//! Dependency injection configuration for the validate_policy feature

use crate::features::load_schema::ports::SchemaStoragePort;
use crate::features::validate_policy::use_case::ValidatePolicyUseCase;
use std::sync::Arc;

/// Factory for building the ValidatePolicyUseCase with its dependencies
pub struct ValidatePolicyUseCaseFactory;

impl ValidatePolicyUseCaseFactory {
    /// Build a new ValidatePolicyUseCase without schema validation
    ///
    /// This creates a use case that only validates policy syntax.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::validate_policy::di::ValidatePolicyUseCaseFactory;
    ///
    /// let use_case = ValidatePolicyUseCaseFactory::build();
    /// let result = use_case.execute(command).await?;
    /// ```
    pub fn build<S: SchemaStoragePort>() -> ValidatePolicyUseCase<S> {
        ValidatePolicyUseCase::new()
    }

    /// Build a new ValidatePolicyUseCase with schema validation
    ///
    /// This creates a use case that validates both syntax and schema compliance.
    ///
    /// # Arguments
    ///
    /// * `schema_storage` - Implementation of SchemaStoragePort for loading schemas
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_policies::features::validate_policy::di::ValidatePolicyUseCaseFactory;
    /// use hodei_policies::infrastructure::SurrealSchemaStorage;
    ///
    /// let storage = Arc::new(SurrealSchemaStorage::new(db_client));
    /// let use_case = ValidatePolicyUseCaseFactory::build_with_schema(storage);
    /// let result = use_case.execute(command).await?;
    /// ```
    pub fn build_with_schema<S: SchemaStoragePort>(
        schema_storage: Arc<S>,
    ) -> ValidatePolicyUseCase<S> {
        ValidatePolicyUseCase::with_schema_storage(schema_storage)
    }
}
