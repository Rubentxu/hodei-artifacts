use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use cedar_policy::{PolicySet, Entity, Policy};
use std::str::FromStr;

use crate::features::evaluate_permissions::dto::{AuthorizationRequest, AuthorizationResponse, AuthorizationDecision};
use crate::features::evaluate_permissions::ports::{
    IamPolicyProvider, OrganizationBoundaryProvider, AuthorizationCache,
    AuthorizationLogger, AuthorizationMetrics, EntityResolver
};
use crate::features::evaluate_permissions::error::{EvaluatePermissionsError, EvaluatePermissionsResult};
use policies::shared::domain::hrn::Hrn;
use hodei_organizations::shared::domain::scp::ServiceControlPolicy;

/// Mock IAM Policy Provider for testing
#[derive(Debug, Default)]
pub struct MockIamPolicyProvider {
    policies: HashMap<String, PolicySet>,
    should_fail: bool,
}

impl MockIamPolicyProvider {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            should_fail: false,
        }
    }

    pub fn with_policy(mut self, principal_hrn: &str, policy_set: PolicySet) -> Self {
        self.policies.insert(principal_hrn.to_string(), policy_set);
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl IamPolicyProvider for MockIamPolicyProvider {
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> EvaluatePermissionsResult<PolicySet> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::IamPolicyProviderError("Mock failure".to_string()));
        }

        let hrn_str = principal_hrn.to_string();
        match self.policies.get(&hrn_str) {
            Some(policy_set) => Ok(policy_set.clone()),
            None => Ok(PolicySet::new()),
        }
    }
}

/// Mock Organization Boundary Provider for testing
#[derive(Debug, Default)]
pub struct MockOrganizationBoundaryProvider {
    scps: HashMap<String, Vec<ServiceControlPolicy>>,
    should_fail: bool,
}

impl MockOrganizationBoundaryProvider {
    pub fn new() -> Self {
        Self {
            scps: HashMap::new(),
            should_fail: false,
        }
    }

    pub fn with_scps(mut self, entity_hrn: &str, scps: Vec<ServiceControlPolicy>) -> Self {
        self.scps.insert(entity_hrn.to_string(), scps);
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl OrganizationBoundaryProvider for MockOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> EvaluatePermissionsResult<PolicySet> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::OrganizationBoundaryProviderError("Mock failure".to_string()));
        }

        let hrn_str = entity_hrn.to_string();
        match self.scps.get(&hrn_str) {
            Some(scps) => {
                let mut policy_set = PolicySet::new();
                for scp in scps {
                    let policy = Policy::from_str(&scp.document)
                        .map_err(|e| EvaluatePermissionsError::PolicyParsingError(e.to_string()))?;
                    policy_set.add(policy).map_err(|e| EvaluatePermissionsError::PolicyParsingError(e.to_string()))?;
                }
                Ok(policy_set)
            }
            None => Ok(PolicySet::new()),
        }
    }
}

/// Mock Authorization Cache for testing
#[derive(Debug, Default)]
pub struct MockAuthorizationCache {
    cache: Arc<Mutex<HashMap<String, AuthorizationResponse>>>,
    should_fail: bool,
}

impl MockAuthorizationCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            should_fail: false,
        }
    }

    pub fn with_cached_response(self, key: &str, response: AuthorizationResponse) -> Self {
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(key.to_string(), response);
        }
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn get_cached(&self, key: &str) -> Option<AuthorizationResponse> {
        let cache = self.cache.lock().unwrap();
        cache.get(key).cloned()
    }

    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }
}

#[async_trait]
impl AuthorizationCache for MockAuthorizationCache {
    async fn get(&self, cache_key: &str) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock cache failure".to_string()));
        }

        let cache = self.cache.lock().unwrap();
        Ok(cache.get(cache_key).cloned())
    }

    async fn put(&self, cache_key: &str, response: &AuthorizationResponse, _ttl: std::time::Duration) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock cache failure".to_string()));
        }

        let mut cache = self.cache.lock().unwrap();
        cache.insert(cache_key.to_string(), response.clone());
        Ok(())
    }

    async fn invalidate_principal(&self, _principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock cache failure".to_string()));
        }
        Ok(())
    }

    async fn invalidate_resource(&self, _resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock cache failure".to_string()));
        }
        Ok(())
    }
}

/// Mock Authorization Logger for testing
#[derive(Debug, Default)]
pub struct MockAuthorizationLogger {
    logged_decisions: Arc<Mutex<Vec<(AuthorizationRequest, AuthorizationResponse)>>>,
    logged_errors: Arc<Mutex<Vec<(AuthorizationRequest, EvaluatePermissionsError)>>>,
    should_fail: bool,
}

impl MockAuthorizationLogger {
    pub fn new() -> Self {
        Self {
            logged_decisions: Arc::new(std::sync::Mutex::new(vec![])),
            logged_errors: Arc::new(std::sync::Mutex::new(vec![])),
            should_fail: false,
        }
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn get_logged_decisions(&self) -> Vec<(AuthorizationRequest, AuthorizationResponse)> {
        let logged = self.logged_decisions.lock().unwrap();
        logged.clone()
    }

    pub fn get_logged_errors(&self) -> Vec<(AuthorizationRequest, EvaluatePermissionsError)> {
        let logged = self.logged_errors.lock().unwrap();
        logged.clone()
    }

    pub fn clear(&self) {
        let mut decisions = self.logged_decisions.lock().unwrap();
        let mut errors = self.logged_errors.lock().unwrap();
        decisions.clear();
        errors.clear();
    }
}

#[async_trait]
impl AuthorizationLogger for MockAuthorizationLogger {
    async fn log_decision(&self, request: &AuthorizationRequest, response: &AuthorizationResponse) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock logger failure".to_string()));
        }

        let mut logged = self.logged_decisions.lock().unwrap();
        logged.push((request.clone(), response.clone()));
        Ok(())
    }

    async fn log_error(&self, request: &AuthorizationRequest, error: &EvaluatePermissionsError) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock logger failure".to_string()));
        }

        let mut logged = self.logged_errors.lock().unwrap();
        logged.push((request.clone(), error.clone()));
        Ok(())
    }
}

/// Mock Authorization Metrics for testing
#[derive(Debug, Default)]
pub struct MockAuthorizationMetrics {
    recorded_decisions: Arc<Mutex<Vec<(AuthorizationDecision, u64)>>>,
    recorded_errors: Arc<Mutex<Vec<String>>>,
    cache_hits: Arc<Mutex<Vec<bool>>>,
    should_fail: bool,
}

impl MockAuthorizationMetrics {
    pub fn new() -> Self {
        Self {
            recorded_decisions: Arc::new(std::sync::Mutex::new(vec![])),
            recorded_errors: Arc::new(std::sync::Mutex::new(vec![])),
            cache_hits: Arc::new(std::sync::Mutex::new(vec![])),
            should_fail: false,
        }
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn get_recorded_decisions(&self) -> Vec<(AuthorizationDecision, u64)> {
        let recorded = self.recorded_decisions.lock().unwrap();
        recorded.clone()
    }

    pub fn get_recorded_errors(&self) -> Vec<String> {
        let recorded = self.recorded_errors.lock().unwrap();
        recorded.clone()
    }

    pub fn get_cache_hits(&self) -> Vec<bool> {
        let hits = self.cache_hits.lock().unwrap();
        hits.clone()
    }

    pub fn clear(&self) {
        let mut decisions = self.recorded_decisions.lock().unwrap();
        let mut errors = self.recorded_errors.lock().unwrap();
        let mut hits = self.cache_hits.lock().unwrap();
        decisions.clear();
        errors.clear();
        hits.clear();
    }
}

#[async_trait]
impl AuthorizationMetrics for MockAuthorizationMetrics {
    async fn record_decision(&self, decision: &AuthorizationDecision, evaluation_time_ms: u64) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock metrics failure".to_string()));
        }

        let mut recorded = self.recorded_decisions.lock().unwrap();
        recorded.push((decision.clone(), evaluation_time_ms));
        Ok(())
    }

    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock metrics failure".to_string()));
        }

        let mut recorded = self.recorded_errors.lock().unwrap();
        recorded.push(error_type.to_string());
        Ok(())
    }

    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::InternalError("Mock metrics failure".to_string()));
        }

        let mut hits = self.cache_hits.lock().unwrap();
        hits.push(hit);
        Ok(())
    }
}

/// Mock Entity Resolver for testing
#[derive(Debug, Default)]
pub struct MockEntityResolver {
    entities: HashMap<String, Entity>,
    should_fail: bool,
}

impl MockEntityResolver {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            should_fail: false,
        }
    }

    pub fn with_entity(mut self, hrn: &str, entity: Entity) -> Self {
        self.entities.insert(hrn.to_string(), entity);
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl EntityResolver for MockEntityResolver {
    async fn resolve_entity(&self, hrn: &Hrn) -> EvaluatePermissionsResult<Entity> {
        if self.should_fail {
            return Err(EvaluatePermissionsError::EntityResolutionError("Mock failure".to_string()));
        }

        let hrn_str = hrn.to_string();
        match self.entities.get(&hrn_str) {
            Some(entity) => Ok(entity.clone()),
            None => {
                // Create a basic entity if not found
                let uid = cedar_policy::EntityUid::from_str(&hrn_str)
                    .map_err(|e| EvaluatePermissionsError::EntityResolutionError(e.to_string()))?;
                let entity = cedar_policy::Entity::new(uid, std::collections::HashMap::new(), std::collections::HashSet::new())
                    .map_err(|e| EvaluatePermissionsError::EntityResolutionError(e.to_string()))?;
                Ok(entity)
            }
        }
    }
}

/// Helper functions for creating test data
pub mod test_helpers {
    use super::*;
    use policies::shared::domain::hrn::Hrn;
    use cedar_policy::PolicyId;

    pub fn create_test_hrn(resource_type: &str, id: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "us-east-1".to_string(),
            "123456789012".to_string(),
            format!("{}/{}", resource_type, id),
        )
    }

    pub fn create_test_scp(id: &str, name: &str, policy: &str) -> ServiceControlPolicy {
        ServiceControlPolicy {
            hrn: create_test_hrn("scp", id),
            name: name.to_string(),
            document: policy.to_string(),
        }
    }

    pub fn create_test_policy_set(policies: &[&str]) -> PolicySet {
        let mut policy_set = PolicySet::new();
        for (i, policy_str) in policies.iter().enumerate() {
            let policy = cedar_policy::Policy::parse(Some(PolicyId::from_str(&format!("policy{}", i)).unwrap()), policy_str)
                .expect("Failed to parse test policy");
            policy_set.add(policy).unwrap();
        }
        policy_set
    }

    pub fn create_allow_all_policy() -> &'static str {
        r#"
        permit(principal, action, resource);
        "#
    }

    pub fn create_deny_all_policy() -> &'static str {
        r#"
        forbid(principal, action, resource);
        "#
    }

    pub fn create_specific_allow_policy(principal: &str, action: &str, resource: &str) -> String {
        format!(
            r#"
            permit(principal == "{}", action == "{}", resource == "{}");
            "#,
            principal, action, resource
        )
    }
}
