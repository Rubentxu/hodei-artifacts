//! Unit tests for EvaluateIamPoliciesUseCase
//!
//! These tests verify the business logic for evaluating IAM policies.
//! They use mocked dependencies to isolate the use case logic.

use crate::features::evaluate_iam_policies::{
    mocks::MockPolicyFinder,
    ports::{PrincipalResolverPort, ResourceResolverPort},
    use_case::EvaluateIamPoliciesUseCase,
};
use kernel::IamPolicyEvaluator;
use kernel::application::ports::authorization::{AuthorizationError, EvaluationRequest};
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet};
use kernel::domain::{Hrn, PolicyId};
use std::sync::Arc;

// Mock implementations for testing
struct MockPrincipalResolver;

#[async_trait::async_trait]
impl PrincipalResolverPort for MockPrincipalResolver {
    async fn resolve_principal(
        &self,
        _hrn: &Hrn,
    ) -> Result<
        Box<dyn kernel::HodeiEntity + Send>,
        crate::features::evaluate_iam_policies::ports::EntityResolverError,
    > {
        // Create a simple mock user entity
        let user = MockUser {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "test-user".to_string(),
            ),
            name: "Test User".to_string(),
        };
        Ok(Box::new(user))
    }
}

struct MockResourceResolver;

#[async_trait::async_trait]
impl ResourceResolverPort for MockResourceResolver {
    async fn resolve_resource(
        &self,
        _hrn: &Hrn,
    ) -> Result<
        Box<dyn kernel::HodeiEntity + Send>,
        crate::features::evaluate_iam_policies::ports::EntityResolverError,
    > {
        // Create a simple mock resource entity
        let resource = MockResource {
            hrn: Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Resource".to_string(),
                "test-resource".to_string(),
            ),
            name: "Test Resource".to_string(),
        };
        Ok(Box::new(resource))
    }
}

struct MockSchemaStorage;

#[async_trait::async_trait]
impl hodei_policies::build_schema::ports::SchemaStoragePort for MockSchemaStorage {
    async fn save_schema(
        &self,
        _schema_json: String,
        _version: Option<String>,
    ) -> Result<String, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok("test-schema-id".to_string())
    }

    async fn get_latest_schema(
        &self,
    ) -> Result<Option<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(Some(r#"{"test": "schema"}"#.to_string()))
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(Some(r#"{"test": "schema"}"#.to_string()))
    }

    async fn delete_schema(
        &self,
        _schema_id: &str,
    ) -> Result<bool, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(true)
    }

    async fn list_schema_versions(
        &self,
    ) -> Result<Vec<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(vec!["v1.0.0".to_string()])
    }
}

// Simple mock entities
#[derive(Debug)]
struct MockUser {
    hrn: Hrn,
    name: String,
}

impl kernel::HodeiEntity for MockUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(
        &self,
    ) -> std::collections::HashMap<kernel::AttributeName, kernel::AttributeValue> {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            kernel::AttributeName::new("name").unwrap(),
            kernel::AttributeValue::string(&self.name),
        );
        attrs
    }
}

#[derive(Debug)]
struct MockResource {
    hrn: Hrn,
    name: String,
}

impl kernel::HodeiEntity for MockResource {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(
        &self,
    ) -> std::collections::HashMap<kernel::AttributeName, kernel::AttributeValue> {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            kernel::AttributeName::new("name").unwrap(),
            kernel::AttributeValue::string(&self.name),
        );
        attrs
    }
}

/// Test that policy evaluation succeeds with valid input
#[tokio::test]
async fn test_evaluate_iam_policies_success() {
    // Setup - Create a mock policy set
    let policy = HodeiPolicy::new(
        PolicyId::new("test-policy"),
        r#"permit(principal, action, resource);"#.to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);
    let mock_policy_finder = Arc::new(MockPolicyFinder::new(policy_set));

    // Create mock resolvers
    let mock_principal_resolver = Arc::new(MockPrincipalResolver);
    let mock_resource_resolver = Arc::new(MockResourceResolver);
    let mock_schema_storage = Arc::new(MockSchemaStorage);

    let use_case = EvaluateIamPoliciesUseCase::new(
        mock_policy_finder,
        mock_principal_resolver,
        mock_resource_resolver,
        mock_schema_storage,
    );

    // Execute
    let request = EvaluationRequest {
        principal_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        ),
        action_name: "read".to_string(),
        resource_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "Resource".to_string(),
            "test-resource".to_string(),
        ),
    };

    let result = use_case.evaluate_iam_policies(request).await;

    // Assert
    assert!(result.is_ok());
    let decision = result.unwrap();
    assert_eq!(decision.action_name, "read");
}

/// Test that policy evaluation fails when policy finder fails
#[tokio::test]
async fn test_evaluate_iam_policies_finder_error() {
    // Setup - Create a mock that returns an error
    let mock_policy_finder = Arc::new(MockPolicyFinder::with_error("Database error".to_string()));
    let mock_principal_resolver = Arc::new(MockPrincipalResolver);
    let mock_resource_resolver = Arc::new(MockResourceResolver);
    let mock_schema_storage = Arc::new(MockSchemaStorage);

    let use_case = EvaluateIamPoliciesUseCase::new(
        mock_policy_finder,
        mock_principal_resolver,
        mock_resource_resolver,
        mock_schema_storage,
    );

    // Execute
    let request = EvaluationRequest {
        principal_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        ),
        action_name: "read".to_string(),
        resource_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "Resource".to_string(),
            "test-resource".to_string(),
        ),
    };

    let result = use_case.evaluate_iam_policies(request).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        AuthorizationError::EvaluationFailed(_) => {} // Expected
        _ => panic!("Expected EvaluationFailed error"),
    }
}

/// Test that policy evaluation handles empty policy set (implicit deny)
#[tokio::test]
async fn test_evaluate_iam_policies_empty_policy_set() {
    // Setup
    let mock_policy_finder = Arc::new(MockPolicyFinder::empty());
    let mock_principal_resolver = Arc::new(MockPrincipalResolver);
    let mock_resource_resolver = Arc::new(MockResourceResolver);
    let mock_schema_storage = Arc::new(MockSchemaStorage);

    let use_case = EvaluateIamPoliciesUseCase::new(
        mock_policy_finder,
        mock_principal_resolver,
        mock_resource_resolver,
        mock_schema_storage,
    );

    // Execute
    let request = EvaluationRequest {
        principal_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        ),
        action_name: "read".to_string(),
        resource_hrn: Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "Resource".to_string(),
            "test-resource".to_string(),
        ),
    };

    let result = use_case.evaluate_iam_policies(request).await;

    // Assert
    assert!(result.is_ok());
    let decision = result.unwrap();
    // With empty policy set, the result should be Denied (default deny)
    assert!(!decision.decision);
    assert!(decision.reason.contains("No IAM policies"));
}
