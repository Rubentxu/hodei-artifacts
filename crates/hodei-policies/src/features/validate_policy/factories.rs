//! Factory functions for the validate_policy feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::load_schema::ports::SchemaStoragePort;
use crate::features::validate_policy::port::ValidatePolicyPort;
use crate::features::validate_policy::use_case::ValidatePolicyUseCase;
use std::sync::Arc;

/// Creates a ValidatePolicyUseCase without schema validation
///
/// This factory creates a use case that only validates policy syntax,
/// without performing schema-based validation.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::validate_policy::factories;
///
/// let use_case = factories::create_validate_policy_use_case_without_schema();
/// let result = use_case.validate(command).await?;
/// ```
pub fn create_validate_policy_use_case_without_schema<S: SchemaStoragePort + 'static>()
-> Arc<dyn ValidatePolicyPort> {
    Arc::new(ValidatePolicyUseCase::<S>::new())
}

/// Creates a ValidatePolicyUseCase with schema validation
///
/// This factory receives an already-constructed schema storage implementation
/// and assembles a use case that validates both syntax and schema compliance.
///
/// # Arguments
///
/// * `schema_storage` - Pre-constructed implementation of SchemaStoragePort
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::validate_policy::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapter
/// let schema_adapter = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Factory receives the adapter and assembles the use case
/// let use_case = factories::create_validate_policy_use_case_with_schema(schema_adapter);
/// let result = use_case.validate(command).await?;
/// ```
pub fn create_validate_policy_use_case_with_schema<S: SchemaStoragePort + 'static>(
    schema_storage: Arc<S>,
) -> Arc<dyn ValidatePolicyPort> {
    Arc::new(ValidatePolicyUseCase::with_schema_storage(schema_storage))
}
