//! Dependency Injection configuration for evaluate_iam_policies feature
//!
//! This module provides factory functions to construct the use case with its dependencies.
//! Following the Dependency Inversion Principle, dependencies are injected as trait objects.

use std::sync::Arc;

use super::ports::{PolicyFinderPort, PrincipalResolverPort, ResourceResolverPort};
use super::use_case::EvaluateIamPoliciesUseCase;
use hodei_policies::features::build_schema::ports::SchemaStoragePort;

/// Factory for building EvaluateIamPoliciesUseCase with dependency injection
///
/// This factory follows the builder pattern to allow flexible construction
/// of the use case with different implementations of its ports.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::evaluate_iam_policies::di::EvaluateIamPoliciesUseCaseFactory;
///
/// let use_case = EvaluateIamPoliciesUseCaseFactory::build(
///     Arc::new(surreal_policy_finder),
///     Arc::new(surreal_principal_resolver),
///     Arc::new(surreal_resource_resolver),
/// );
/// ```
pub struct EvaluateIamPoliciesUseCaseFactory;

impl EvaluateIamPoliciesUseCaseFactory {
    /// Build the use case with injected dependencies
    ///
    /// This is the primary factory method for constructing the use case.
    /// All dependencies are injected as trait objects, allowing for
    /// maximum flexibility and testability.
    ///
    /// # Arguments
    ///
    /// * `policy_finder` - Implementation of PolicyFinderPort for retrieving policies
    /// * `principal_resolver` - Implementation of PrincipalResolverPort for resolving principals
    /// * `resource_resolver` - Implementation of ResourceResolverPort for resolving resources
    /// * `schema_storage` - Implementation of SchemaStoragePort for schema loading
    ///
    /// # Returns
    ///
    /// A fully configured `EvaluateIamPoliciesUseCase` ready for use
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_iam::features::evaluate_iam_policies::di::EvaluateIamPoliciesUseCaseFactory;
    /// use hodei_iam::infrastructure::surreal::{
    ///     SurrealPolicyFinderAdapter,
    ///     SurrealPrincipalResolverAdapter,
    ///     SurrealResourceResolverAdapter,
    /// };
    ///
    /// // In the composition root (e.g., main.rs or app_state.rs)
    /// let db = /* SurrealDB connection */;
    ///
    /// let policy_finder = Arc::new(SurrealPolicyFinderAdapter::new(db.clone()));
    /// let principal_resolver = Arc::new(SurrealPrincipalResolverAdapter::new(db.clone()));
    /// let resource_resolver = Arc::new(SurrealResourceResolverAdapter::new(db.clone()));
    /// let schema_storage = Arc::new(SurrealSchemaStorage::new(db.clone()));
    ///
    /// let use_case = EvaluateIamPoliciesUseCaseFactory::build(
    ///     policy_finder,
    ///     principal_resolver,
    ///     resource_resolver,
    ///     schema_storage,
    /// );
    /// ```
    pub fn build<PF, PR, RR>(
        policy_finder: Arc<PF>,
        principal_resolver: Arc<PR>,
        resource_resolver: Arc<RR>,
        schema_storage: Arc<dyn SchemaStoragePort>,
    ) -> EvaluateIamPoliciesUseCase<PF, PR, RR>
    where
        PF: PolicyFinderPort,
        PR: PrincipalResolverPort,
        RR: ResourceResolverPort,
    {
        EvaluateIamPoliciesUseCase::new(
            policy_finder,
            principal_resolver,
            resource_resolver,
            schema_storage,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hodei_policies::features::build_schema::error::BuildSchemaError;
    use kernel::Hrn;
    use kernel::application::ports::authorization::IamPolicyEvaluator;
    use kernel::domain::HodeiPolicySet;

    // Import mocks from use_case module
    use crate::features::evaluate_iam_policies::use_case::tests::mocks::{
        MockDocument, MockPolicyFinder, MockPrincipalResolver, MockResourceResolver, MockUser,
    };

    // Mock schema storage for testing
    struct MockSchemaStorage;

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            _schema: String,
            _version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            Ok("mock-schema-id".to_string())
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
            Ok(false)
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_factory_builds_use_case_successfully() {
        // Arrange
        let mock_policy_finder = Arc::new(MockPolicyFinder::new(HodeiPolicySet::new()));
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        // Act
        let mock_schema_storage = Arc::new(MockSchemaStorage);
        let use_case = EvaluateIamPoliciesUseCaseFactory::build(
            mock_policy_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            mock_schema_storage,
        );

        // Assert - use the use case to verify it's functional
        let request = kernel::application::ports::authorization::EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        let result = use_case.evaluate_iam_policies(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_with_different_implementations() {
        // Arrange - Use error-producing mocks to test flexibility
        let mock_policy_finder = Arc::new(MockPolicyFinder::with_error());
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        // Act
        let mock_schema_storage = Arc::new(MockSchemaStorage);
        let use_case = EvaluateIamPoliciesUseCaseFactory::build(
            mock_policy_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            mock_schema_storage,
        );

        // Assert - verify the use case handles errors from the injected mock
        let request = kernel::application::ports::authorization::EvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        let result = use_case.evaluate_iam_policies(request).await;
        assert!(result.is_err(), "Expected error from mock policy finder");
    }
}
