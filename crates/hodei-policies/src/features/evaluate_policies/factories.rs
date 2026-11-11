//! Factory functions for the evaluate_policies feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::build_schema::ports::SchemaStoragePort;
use crate::features::evaluate_policies::ports::EvaluatePoliciesPort;
use crate::features::evaluate_policies::use_case::EvaluatePoliciesUseCase;
use std::sync::Arc;

/// Creates an EvaluatePoliciesUseCase with the provided dependencies
///
/// This factory receives an already-constructed schema storage implementation
/// and assembles a use case for evaluating authorization policies.
///
/// # Arguments
///
/// * `schema_storage` - Pre-constructed implementation of SchemaStoragePort
///
/// # Returns
///
/// An `Arc<dyn EvaluatePoliciesPort>` trait object, enabling dependency inversion
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::evaluate_policies::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapter
/// let schema_adapter = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Factory receives the adapter and assembles the use case
/// let use_case = factories::create_evaluate_policies_use_case(schema_adapter);
/// let decision = use_case.evaluate(command).await?;
/// ```
pub fn create_evaluate_policies_use_case(
    schema_storage: Arc<dyn SchemaStoragePort>,
) -> Arc<dyn EvaluatePoliciesPort> {
    Arc::new(EvaluatePoliciesUseCase::new(schema_storage))
}

/// Build an evaluation use case that falls back to one named schema.
///
/// A request that names a schema still gets that one; a request that names
/// none gets the schema the deployment declares authoritative.
pub fn create_evaluate_policies_use_case_with_authoritative_schema(
    schema_storage: Arc<dyn SchemaStoragePort>,
    authoritative_schema_id: impl Into<String>,
) -> Arc<dyn EvaluatePoliciesPort> {
    Arc::new(EvaluatePoliciesUseCase::with_authoritative_schema(
        schema_storage,
        authoritative_schema_id,
    ))
}
