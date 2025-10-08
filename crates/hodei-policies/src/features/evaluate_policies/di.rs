//! Dependency Injection for the evaluate_policies feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::ports::EvaluatePoliciesPort;
use super::use_case::EvaluatePoliciesUseCase;
use crate::features::build_schema::ports::SchemaStoragePort;
use std::sync::Arc;

/// Factory for creating EvaluatePoliciesUseCase instances
pub struct EvaluatePoliciesUseCaseFactory;

impl EvaluatePoliciesUseCaseFactory {
    /// Creates a new EvaluatePoliciesUseCase instance as a trait object
    ///
    /// Returns an Arc<dyn EvaluatePoliciesPort> to enable dependency inversion.
    /// This allows other bounded contexts to depend on the port trait
    /// rather than the concrete implementation.
    ///
    /// # Arguments
    ///
    /// * `schema_storage` - Implementation of SchemaStoragePort for loading schemas
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::evaluate_policies::di::EvaluatePoliciesUseCaseFactory;
    /// use hodei_policies::infrastructure::surreal_schema_adapter::SurrealSchemaAdapter;
    ///
    /// let schema_storage = Arc::new(SurrealSchemaAdapter::new(db_client));
    /// let evaluator = EvaluatePoliciesUseCaseFactory::build(schema_storage);
    /// let decision = evaluator.evaluate(command).await?;
    /// ```
    pub fn build(schema_storage: Arc<dyn SchemaStoragePort>) -> Arc<dyn EvaluatePoliciesPort> {
        Arc::new(EvaluatePoliciesUseCase::new(schema_storage))
    }
}
