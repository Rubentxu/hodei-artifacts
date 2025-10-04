use hodei_authorizer::ports::{IamPolicyProvider, OrganizationBoundaryProvider, AuthorizationError};
use hodei_authorizer::authorizer::AuthorizerService;
use policies::shared::domain::policy::{PolicySet, Policy};
use policies::shared::domain::hrn::Hrn;
use policies::shared::application::engine::{AuthorizationRequest, AuthorizationResponse, PolicyEvaluator, Decision};
use hodei_organizations::shared::domain::ServiceControlPolicy;
use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of IamPolicyProvider for testing
#[derive(Debug, Default)]
pub struct MockIamPolicyProvider {
    policies: RwLock<HashMap<String, PolicySet>>,
}

impl MockIamPolicyProvider {
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_policy(mut self, principal_hrn: Hrn, policy: Policy) -> Self {
        let mut policies = self.policies.write().unwrap();
        let policy_set = policies.entry(principal_hrn.to_string()).or_insert_with(PolicySet::new);
        policy_set.add_policy(policy);
        self
    }
}

#[async_trait]
impl IamPolicyProvider for MockIamPolicyProvider {
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError> {
        let policies = self.policies.read().unwrap();
        Ok(policies.get(&principal_hrn.to_string()).cloned().unwrap_or_else(PolicySet::new))
    }
}

/// Mock implementation of OrganizationBoundaryProvider for testing
#[derive(Debug, Default)]
pub struct MockOrganizationBoundaryProvider {
    scps: RwLock<HashMap<String, Vec<ServiceControlPolicy>>>,
}

impl MockOrganizationBoundaryProvider {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(mut self, entity_hrn: Hrn, scp: ServiceControlPolicy) -> Self {
        let mut scps = self.scps.write().unwrap();
        let scp_list = scps.entry(entity_hrn.to_string()).or_insert_with(Vec::new);
        scp_list.push(scp);
        self
    }
}

#[async_trait]
impl OrganizationBoundaryProvider for MockOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, AuthorizationError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&entity_hrn.to_string()).cloned().unwrap_or_else(Vec::new))
    }
}

#[tokio::test]
async fn test_allow_requires_both_iam_and_scp_allow() {
    // Arrange
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(
            Hrn::new("user", "test-user"),
            Policy::from_str("permit(principal, action::\"s3:GetObject\", resource);").unwrap(),
        );
    
    let org_provider = MockOrganizationBoundaryProvider::new()
        .with_scp(
            Hrn::new("account", "test-account"),
            ServiceControlPolicy::new(
                Hrn::new("scp", "allow-scp"),
                "AllowSCP".to_string(),
                "permit(principal, action::\"s3:GetObject\", resource);".to_string(),
            ),
        );
    
    let policy_evaluator = PolicyEvaluator::new();
    let authorizer = AuthorizerService::new(iam_provider, org_provider, policy_evaluator);
    
    let request = AuthorizationRequest {
        principal: Hrn::new("user", "test-user"),
        action: "s3:GetObject".to_string(),
        resource: Hrn::new("resource", "test-resource"),
    };

    // Act
    let result = authorizer.is_authorized(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision(), Decision::Allow);
}

#[tokio::test]
async fn test_deny_when_iam_allows_but_scp_denies() {
    // Arrange
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(
            Hrn::new("user", "test-user"),
            Policy::from_str("permit(principal, action::\"s3:GetObject\", resource);").unwrap(),
        );
    
    let org_provider = MockOrganizationBoundaryProvider::new()
        .with_scp(
            Hrn::new("account", "test-account"),
            ServiceControlPolicy::new(
                Hrn::new("scp", "deny-scp"),
                "DenySCP".to_string(),
                "forbid(principal, action::\"s3:GetObject\", resource);".to_string(),
            ),
        );
    
    let policy_evaluator = PolicyEvaluator::new();
    let authorizer = AuthorizerService::new(iam_provider, org_provider, policy_evaluator);
    
    let request = AuthorizationRequest {
        principal: Hrn::new("user", "test-user"),
        action: "s3:GetObject".to_string(),
        resource: Hrn::new("resource", "test-resource"),
    };

    // Act
    let result = authorizer.is_authorized(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision(), Decision::Deny);
}

#[tokio::test]
async fn test_deny_when_iam_denies_but_scp_allows() {
    // Arrange
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(
            Hrn::new("user", "test-user"),
            Policy::from_str("forbid(principal, action::\"s3:GetObject\", resource);").unwrap(),
        );
    
    let org_provider = MockOrganizationBoundaryProvider::new()
        .with_scp(
            Hrn::new("account", "test-account"),
            ServiceControlPolicy::new(
                Hrn::new("scp", "allow-scp"),
                "AllowSCP".to_string(),
                "permit(principal, action::\"s3:GetObject\", resource);".to_string(),
            ),
        );
    
    let policy_evaluator = PolicyEvaluator::new();
    let authorizer = AuthorizerService::new(iam_provider, org_provider, policy_evaluator);
    
    let request = AuthorizationRequest {
        principal: Hrn::new("user", "test-user"),
        action: "s3:GetObject".to_string(),
        resource: Hrn::new("resource", "test-resource"),
    };

    // Act
    let result = authorizer.is_authorized(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision(), Decision::Deny);
}
