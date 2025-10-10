//! Use case for evaluating IAM policies
//!
//! This use case implements the `IamPolicyEvaluator` trait from the kernel,
//! making hodei-iam responsible for coordinating IAM policy evaluation.
//!
//! # Architecture
//!
//! This follows the Vertical Slice Architecture (VSA) pattern:
//! - Uses segregated ports for dependencies (PolicyFinderPort, PrincipalResolverPort, ResourceResolverPort)
//! - **Delegates Cedar evaluation to hodei-policies crate** (following bounded context rules)
//! - Implements the cross-context trait from kernel
//!
//! # Delegation Strategy
//!
//! This use case does NOT implement Cedar evaluation logic directly. Instead:
//! 1. Retrieves effective IAM policies for the principal
//! 2. Resolves principal and resource entities
//! 3. Delegates to `hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase`
//! 4. Maps the result back to kernel types
//!
//! This ensures zero coupling to Cedar and respects bounded context boundaries.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

use kernel::application::ports::authorization::{
    AuthorizationError, EvaluationDecision as KernelEvaluationDecision,
    EvaluationRequest as KernelEvaluationRequest, IamPolicyEvaluator,
};

use super::ports::{
    EntityResolverError, PolicyFinderError, PolicyFinderPort, PrincipalResolverPort,
    ResourceResolverPort,
};

// Import types from hodei-policies for delegation
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use hodei_policies::features::evaluate_policies::{
    EvaluatePoliciesUseCase,
    dto::{AuthorizationRequest, Decision, EvaluatePoliciesCommand},
};

/// Use case for evaluating IAM policies
///
/// This use case coordinates the evaluation of IAM policies to determine
/// if a principal (user, service account) has permission to perform
/// an action on a resource.
///
/// # Process
///
/// 1. Retrieve effective policies for the principal via `PolicyFinderPort`
/// 2. Resolve principal entity via `PrincipalResolverPort`
/// 3. Resolve resource entity via `ResourceResolverPort`
/// 4. Delegate evaluation to `hodei_policies::EvaluatePoliciesUseCase`
/// 5. Map result back to kernel types
/// 6. Return authorization decision
///
pub struct EvaluateIamPoliciesUseCase {
    /// Port for retrieving effective policies
    policy_finder: Arc<dyn PolicyFinderPort>,

    /// Port for resolving principal entities
    principal_resolver: Arc<dyn PrincipalResolverPort>,

    /// Port for resolving resource entities
    resource_resolver: Arc<dyn ResourceResolverPort>,

    /// Use case from hodei-policies for Cedar evaluation
    policies_evaluator: EvaluatePoliciesUseCase,
}

impl EvaluateIamPoliciesUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_finder` - Port for retrieving effective policies
    /// * `principal_resolver` - Port for resolving principal entities
    /// * `resource_resolver` - Port for resolving resource entities
    /// * `schema_storage` - Port for schema storage (required by policies evaluator)
    pub fn new(
        policy_finder: Arc<dyn PolicyFinderPort>,
        principal_resolver: Arc<dyn PrincipalResolverPort>,
        resource_resolver: Arc<dyn ResourceResolverPort>,
        schema_storage: Arc<dyn SchemaStoragePort>,
    ) -> Self {
        Self {
            policy_finder,
            principal_resolver,
            resource_resolver,
            policies_evaluator: EvaluatePoliciesUseCase::new(schema_storage),
        }
    }
}

#[async_trait]
impl IamPolicyEvaluator for EvaluateIamPoliciesUseCase {
    #[instrument(
        skip(self, request),
        fields(
            principal_hrn = %request.principal_hrn,
            action = %request.action_name,
            resource_hrn = %request.resource_hrn
        )
    )]
    async fn evaluate_iam_policies(
        &self,
        request: KernelEvaluationRequest,
    ) -> Result<KernelEvaluationDecision, AuthorizationError> {
        info!("Starting IAM policy evaluation");

        // Step 1: Retrieve effective IAM policies for the principal
        debug!("Retrieving effective policies for principal");
        let policy_set = self
            .policy_finder
            .get_effective_policies(&request.principal_hrn)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to retrieve policies");
                Self::map_policy_finder_error(e)
            })?;

        debug!(
            policy_count = policy_set.policies().len(),
            "Retrieved effective policies"
        );

        // Check if there are any policies (implicit deny if none)
        if policy_set.policies().is_empty() {
            warn!("No policies found for principal, denying by default (implicit deny)");
            return Ok(KernelEvaluationDecision {
                principal_hrn: request.principal_hrn.clone(),
                action_name: request.action_name.clone(),
                resource_hrn: request.resource_hrn.clone(),
                decision: false,
                reason: "No IAM policies found for principal (implicit deny)".to_string(),
            });
        }

        // Step 2: Resolve principal entity
        debug!("Resolving principal entity");
        let principal_entity = self
            .principal_resolver
            .resolve_principal(&request.principal_hrn)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to resolve principal");
                Self::map_entity_resolver_error(e)
            })?;

        debug!("Principal entity resolved successfully");

        // Step 3: Resolve resource entity
        debug!("Resolving resource entity");
        let resource_entity = self
            .resource_resolver
            .resolve_resource(&request.resource_hrn)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to resolve resource");
                Self::map_entity_resolver_error(e)
            })?;

        debug!("Resource entity resolved successfully");

        // Step 4: Build authorization request for hodei-policies
        let principal_ref = principal_entity.as_ref();
        let resource_ref = resource_entity.as_ref();
        let entities: Vec<&dyn kernel::HodeiEntity> = vec![principal_ref, resource_ref];

        let auth_request = AuthorizationRequest {
            principal: principal_ref,
            action: &request.action_name,
            resource: resource_ref,
            context: None, // TODO: Support context if needed
        };

        let evaluate_command = EvaluatePoliciesCommand::new(auth_request, &policy_set, &entities);

        // Step 5: Delegate evaluation to hodei-policies
        debug!("Delegating evaluation to hodei-policies");
        let evaluation_result = self
            .policies_evaluator
            .execute(evaluate_command)
            .await
            .map_err(|e| {
                warn!(error = %e, "Policy evaluation failed");
                AuthorizationError::EvaluationFailed(format!("Cedar evaluation failed: {}", e))
            })?;

        // Step 6: Map result to kernel types
        let decision = matches!(evaluation_result.decision, Decision::Allow);
        let reason = if evaluation_result.reasons.is_empty() {
            if decision {
                "Access allowed by IAM policies".to_string()
            } else {
                "Access denied by IAM policies".to_string()
            }
        } else {
            evaluation_result.reasons.join("; ")
        };

        info!(
            decision = decision,
            determining_policies = ?evaluation_result.determining_policies,
            "IAM policy evaluation completed"
        );

        Ok(KernelEvaluationDecision {
            principal_hrn: request.principal_hrn.clone(),
            action_name: request.action_name.to_string(),
            resource_hrn: request.resource_hrn.clone(),
            decision,
            reason,
        })
    }
}

impl EvaluateIamPoliciesUseCase {
    /// Map PolicyFinderError to AuthorizationError
    fn map_policy_finder_error(error: PolicyFinderError) -> AuthorizationError {
        match error {
            PolicyFinderError::PrincipalNotFound(msg) => {
                AuthorizationError::EvaluationFailed(format!("Principal not found: {}", msg))
            }
            PolicyFinderError::RepositoryError(msg) => {
                AuthorizationError::EvaluationFailed(format!("Repository error: {}", msg))
            }
            PolicyFinderError::PolicyParseError(msg) => {
                AuthorizationError::EvaluationFailed(format!("Policy parse error: {}", msg))
            }
            PolicyFinderError::InternalError(msg) => {
                AuthorizationError::EvaluationFailed(format!("Internal error: {}", msg))
            }
        }
    }

    /// Map EntityResolverError to AuthorizationError
    fn map_entity_resolver_error(error: EntityResolverError) -> AuthorizationError {
        match error {
            EntityResolverError::EntityNotFound(msg) => {
                AuthorizationError::EvaluationFailed(format!("Entity not found: {}", msg))
            }
            EntityResolverError::InvalidHrn(msg) => {
                AuthorizationError::EvaluationFailed(format!("Invalid HRN: {}", msg))
            }
            EntityResolverError::UnsupportedEntityType(msg) => {
                AuthorizationError::EvaluationFailed(format!("Unsupported entity type: {}", msg))
            }
            EntityResolverError::RepositoryError(msg) => {
                AuthorizationError::EvaluationFailed(format!("Repository error: {}", msg))
            }
            EntityResolverError::InternalError(msg) => {
                AuthorizationError::EvaluationFailed(format!("Internal error: {}", msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;
    use kernel::domain::{HodeiPolicy, HodeiPolicySet, PolicyId};

    // Mock implementations for testing
    mod mocks {
        use super::*;

        use kernel::{
            AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType,
            ResourceTypeName, ServiceName,
        };
        use std::collections::HashMap;

        pub struct MockPolicyFinder {
            policy_set: HodeiPolicySet,
            should_error: bool,
        }

        impl MockPolicyFinder {
            pub fn new(policy_set: HodeiPolicySet) -> Self {
                Self {
                    policy_set,
                    should_error: false,
                }
            }

            pub fn with_error() -> Self {
                Self {
                    policy_set: HodeiPolicySet::new(vec![]),
                    should_error: true,
                }
            }
        }

        #[async_trait]
        impl PolicyFinderPort for MockPolicyFinder {
            async fn get_effective_policies(
                &self,
                _principal_hrn: &Hrn,
            ) -> Result<HodeiPolicySet, PolicyFinderError> {
                if self.should_error {
                    return Err(PolicyFinderError::RepositoryError("Mock error".to_string()));
                }
                Ok(self.policy_set.clone())
            }
        }

        #[derive(Debug)]
        pub struct MockUser {
            pub hrn: Hrn,
            pub name: String,
        }

        impl HodeiEntityType for MockUser {
            fn service_name() -> ServiceName {
                ServiceName::new("iam").unwrap()
            }

            fn resource_type_name() -> ResourceTypeName {
                ResourceTypeName::new("User").unwrap()
            }

            fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
                vec![(AttributeName::new("name").unwrap(), AttributeType::string())]
            }
        }

        impl HodeiEntity for MockUser {
            fn hrn(&self) -> &Hrn {
                &self.hrn
            }

            fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
                let mut attrs = HashMap::new();
                attrs.insert(
                    AttributeName::new("name").unwrap(),
                    AttributeValue::string(&self.name),
                );
                attrs
            }
        }

        #[derive(Debug)]
        pub struct MockDocument {
            pub hrn: Hrn,
            pub title: String,
        }

        impl HodeiEntityType for MockDocument {
            fn service_name() -> ServiceName {
                ServiceName::new("storage").unwrap()
            }

            fn resource_type_name() -> ResourceTypeName {
                ResourceTypeName::new("Document").unwrap()
            }

            fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
                vec![(
                    AttributeName::new("title").unwrap(),
                    AttributeType::string(),
                )]
            }
        }

        impl HodeiEntity for MockDocument {
            fn hrn(&self) -> &Hrn {
                &self.hrn
            }

            fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
                let mut attrs = HashMap::new();
                attrs.insert(
                    AttributeName::new("title").unwrap(),
                    AttributeValue::string(&self.title),
                );
                attrs
            }
        }

        pub struct MockPrincipalResolver {
            #[allow(dead_code)]
            entity: Option<Box<dyn HodeiEntity + Send>>,
            should_error: bool,
        }

        impl MockPrincipalResolver {
            pub fn new(entity: Box<dyn HodeiEntity + Send>) -> Self {
                Self {
                    entity: Some(entity),
                    should_error: false,
                }
            }

            pub fn with_error() -> Self {
                Self {
                    entity: None,
                    should_error: true,
                }
            }
        }

        #[async_trait]
        impl PrincipalResolverPort for MockPrincipalResolver {
            async fn resolve_principal(
                &self,
                _hrn: &Hrn,
            ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError> {
                if self.should_error {
                    return Err(EntityResolverError::EntityNotFound(
                        "Mock error".to_string(),
                    ));
                }
                // Clone the entity for testing
                let user = MockUser {
                    hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
                    name: "Alice".to_string(),
                };
                Ok(Box::new(user))
            }
        }

        pub struct MockResourceResolver {
            #[allow(dead_code)]
            entity: Option<Box<dyn HodeiEntity + Send>>,
            should_error: bool,
        }

        impl MockResourceResolver {
            pub fn new(entity: Box<dyn HodeiEntity + Send>) -> Self {
                Self {
                    entity: Some(entity),
                    should_error: false,
                }
            }

            pub fn with_error() -> Self {
                Self {
                    entity: None,
                    should_error: true,
                }
            }
        }

        #[async_trait]
        impl ResourceResolverPort for MockResourceResolver {
            async fn resolve_resource(
                &self,
                _hrn: &Hrn,
            ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError> {
                if self.should_error {
                    return Err(EntityResolverError::EntityNotFound(
                        "Mock error".to_string(),
                    ));
                }
                // Clone the entity for testing
                let doc = MockDocument {
                    hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
                    title: "Document 1".to_string(),
                };
                Ok(Box::new(doc))
            }
        }
    }

    use mocks::*;

    #[tokio::test]
    async fn test_evaluate_denies_when_no_policies() {
        // Arrange
        let mock_finder = Arc::new(MockPolicyFinder::new(HodeiPolicySet::new(vec![])));
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        let use_case = EvaluateIamPoliciesUseCase::new(
            mock_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            Arc::new(MockSchemaStorage::new()),
        );

        let request = KernelEvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert!(!decision.decision, "Expected deny decision (implicit deny)");
        assert!(decision.reason.contains("No IAM policies"));
    }

    #[tokio::test]
    async fn test_evaluate_allows_when_permit_policy_exists() {
        // Arrange
        let policy_text = r#"permit(principal, action, resource);"#;
        let policy = HodeiPolicy::new(PolicyId::new("test-policy"), policy_text.to_string());
        let policy_set = HodeiPolicySet::new(vec![policy]);

        let mock_finder = Arc::new(MockPolicyFinder::new(policy_set));
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        let use_case = EvaluateIamPoliciesUseCase::new(
            mock_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            Arc::new(MockSchemaStorage::new()),
        );

        let request = KernelEvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert!(decision.decision, "Expected allow decision");
    }

    #[tokio::test]
    async fn test_evaluate_handles_policy_retrieval_error() {
        // Arrange
        let mock_finder = Arc::new(MockPolicyFinder::with_error());
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        let use_case = EvaluateIamPoliciesUseCase::new(
            mock_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            Arc::new(MockSchemaStorage::new()),
        );

        let request = KernelEvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, AuthorizationError::EvaluationFailed(_)));
    }

    #[tokio::test]
    async fn test_evaluate_handles_principal_resolution_error() {
        // Arrange
        let policy_text = r#"permit(principal, action, resource);"#;
        let policy = HodeiPolicy::new(PolicyId::new("test-policy"), policy_text.to_string());
        let policy_set = HodeiPolicySet::new(vec![policy]);

        let mock_finder = Arc::new(MockPolicyFinder::new(policy_set));
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::with_error());
        let mock_resource_resolver = Arc::new(MockResourceResolver::new(Box::new(MockDocument {
            hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
            title: "Doc1".to_string(),
        })));

        let use_case = EvaluateIamPoliciesUseCase::new(
            mock_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            Arc::new(MockSchemaStorage::new()),
        );

        let request = KernelEvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, AuthorizationError::EvaluationFailed(_)));
    }

    #[tokio::test]
    async fn test_evaluate_handles_resource_resolution_error() {
        // Arrange
        let policy_text = r#"permit(principal, action, resource);"#;
        let policy = HodeiPolicy::new(PolicyId::new("test-policy"), policy_text.to_string());
        let policy_set = HodeiPolicySet::new(vec![policy]);

        let mock_finder = Arc::new(MockPolicyFinder::new(policy_set));
        let mock_principal_resolver = Arc::new(MockPrincipalResolver::new(Box::new(MockUser {
            hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            name: "Alice".to_string(),
        })));
        let mock_resource_resolver = Arc::new(MockResourceResolver::with_error());

        let use_case = EvaluateIamPoliciesUseCase::new(
            mock_finder,
            mock_principal_resolver,
            mock_resource_resolver,
            Arc::new(MockSchemaStorage::new()),
        );

        let request = KernelEvaluationRequest {
            principal_hrn: Hrn::from_string("hrn:hodei:iam::account123:user/alice").unwrap(),
            action_name: "Read".to_string(),
            resource_hrn: Hrn::from_string("hrn:hodei:artifact::account123:artifact/doc1").unwrap(),
        };

        // Act
        let result = use_case.evaluate_iam_policies(request).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, AuthorizationError::EvaluationFailed(_)));
    }

    // Mock SchemaStorage for testing
    struct MockSchemaStorage;

    impl MockSchemaStorage {
        pub fn new() -> Self {
            Self
        }
    }

    #[async_trait]
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
}
