//! Factory functions for the playground_evaluate feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::playground_evaluate::ports::{
    ContextConverterPort, PlaygroundEvaluatePort, PolicyEvaluatorPort, PolicyValidatorPort,
    SchemaLoaderPort,
};
use crate::features::playground_evaluate::use_case::PlaygroundEvaluateUseCase;
use std::sync::Arc;

/// Creates a PlaygroundEvaluateUseCase with the provided dependencies
///
/// This factory receives already-constructed implementations of all required ports
/// and assembles a use case for playground policy evaluation.
///
/// # Arguments
///
/// * `schema_loader` - Pre-constructed implementation of SchemaLoaderPort
/// * `policy_validator` - Pre-constructed implementation of PolicyValidatorPort
/// * `policy_evaluator` - Pre-constructed implementation of PolicyEvaluatorPort
/// * `context_converter` - Pre-constructed implementation of ContextConverterPort
///
/// # Returns
///
/// An `Arc<dyn PlaygroundEvaluatePort>` trait object, enabling dependency inversion
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::playground_evaluate::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapters
/// let schema_loader = Arc::new(InlineSchemaLoader::new());
/// let policy_validator = Arc::new(CedarPolicyValidator::new());
/// let policy_evaluator = Arc::new(CedarPolicyEvaluator::new());
/// let context_converter = Arc::new(JsonContextConverter::new());
///
/// // Factory receives the adapters and assembles the use case
/// let use_case = factories::create_playground_evaluate_use_case(
///     schema_loader,
///     policy_validator,
///     policy_evaluator,
///     context_converter,
/// );
///
/// let result = use_case.execute(command).await?;
/// ```
pub fn create_playground_evaluate_use_case(
    schema_loader: Arc<dyn SchemaLoaderPort>,
    policy_validator: Arc<dyn PolicyValidatorPort>,
    policy_evaluator: Arc<dyn PolicyEvaluatorPort>,
    context_converter: Arc<dyn ContextConverterPort>,
) -> Arc<dyn PlaygroundEvaluatePort> {
    Arc::new(PlaygroundEvaluateUseCase::new(
        schema_loader,
        policy_validator,
        policy_evaluator,
        context_converter,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::mocks::{
        MockContextConverter, MockPolicyEvaluator, MockPolicyValidator, MockSchemaLoader,
    };
    use super::*;

    #[test]
    fn test_factory_builds_use_case_with_all_dependencies() {
        let schema_loader = Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator = Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator = Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter = Arc::new(MockContextConverter::new());

        let _use_case = create_playground_evaluate_use_case(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // If we get here, the factory successfully created the use case
    }

    #[test]
    fn test_factory_accepts_trait_objects() {
        // This test verifies that the factory accepts trait objects
        // and doesn't require concrete types, enabling full testability

        let schema_loader: Arc<dyn SchemaLoaderPort> =
            Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator: Arc<dyn PolicyValidatorPort> =
            Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator: Arc<dyn PolicyEvaluatorPort> =
            Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter: Arc<dyn ContextConverterPort> =
            Arc::new(MockContextConverter::new());

        let _use_case = create_playground_evaluate_use_case(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Success: factory works with trait objects
    }
}
