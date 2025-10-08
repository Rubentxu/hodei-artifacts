//! Dependency Injection for the playground_evaluate feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::use_case::PlaygroundEvaluateUseCase;
use std::sync::Arc;

/// Factory for creating PlaygroundEvaluateUseCase instances
pub struct PlaygroundEvaluateUseCaseFactory;

impl PlaygroundEvaluateUseCaseFactory {
    /// Creates a new PlaygroundEvaluateUseCase instance with injected dependencies
    ///
    /// This factory accepts all required ports as trait objects, enabling
    /// full testability and flexibility in implementation.
    ///
    /// # Arguments
    ///
    /// * `schema_loader` - Port for loading schemas (inline or from storage)
    /// * `policy_validator` - Port for validating policies against schemas
    /// * `policy_evaluator` - Port for evaluating authorization requests
    /// * `context_converter` - Port for converting context attributes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_policies::features::playground_evaluate::di::PlaygroundEvaluateUseCaseFactory;
    /// use hodei_policies::features::playground_evaluate::ports::{
    ///     SchemaLoaderPort, PolicyValidatorPort, PolicyEvaluatorPort, ContextConverterPort
    /// };
    ///
    /// let schema_loader = Arc::new(MySchemaLoader::new());
    /// let policy_validator = Arc::new(MyPolicyValidator::new());
    /// let policy_evaluator = Arc::new(MyPolicyEvaluator::new());
    /// let context_converter = Arc::new(MyContextConverter::new());
    ///
    /// let use_case = PlaygroundEvaluateUseCaseFactory::build(
    ///     schema_loader,
    ///     policy_validator,
    ///     policy_evaluator,
    ///     context_converter,
    /// );
    ///
    /// let command = PlaygroundEvaluateCommand::new_with_inline_schema(...);
    /// let result = use_case.execute(command).await?;
    /// ```
    pub fn build(
        schema_loader: Arc<dyn super::ports::SchemaLoaderPort>,
        policy_validator: Arc<dyn super::ports::PolicyValidatorPort>,
        policy_evaluator: Arc<dyn super::ports::PolicyEvaluatorPort>,
        context_converter: Arc<dyn super::ports::ContextConverterPort>,
    ) -> PlaygroundEvaluateUseCase {
        PlaygroundEvaluateUseCase::new(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        )
    }
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

        let _use_case = PlaygroundEvaluateUseCaseFactory::build(
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

        let schema_loader: Arc<dyn super::super::ports::SchemaLoaderPort> =
            Arc::new(MockSchemaLoader::new_with_success());
        let policy_validator: Arc<dyn super::super::ports::PolicyValidatorPort> =
            Arc::new(MockPolicyValidator::new_with_success());
        let policy_evaluator: Arc<dyn super::super::ports::PolicyEvaluatorPort> =
            Arc::new(MockPolicyEvaluator::new_with_allow());
        let context_converter: Arc<dyn super::super::ports::ContextConverterPort> =
            Arc::new(MockContextConverter::new());

        let _use_case = PlaygroundEvaluateUseCaseFactory::build(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        );

        // Success: factory works with trait objects
    }
}
