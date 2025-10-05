use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::features::evaluate_permissions::dto::{
    AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
};
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics, EntityResolverError,
    EntityResolverPort,
};
use ::kernel::Hrn;
use cedar_policy::EntityUid;

/// Mock Authorization Cache for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationCache {
    responses: Arc<Mutex<std::collections::HashMap<String, AuthorizationResponse>>>,
}

impl MockAuthorizationCache {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn with_response(self, cache_key: &str, response: AuthorizationResponse) -> Self {
        let mut responses = self.responses.lock().unwrap();
        responses.insert(cache_key.to_string(), response);
        drop(responses);
        self
    }
}

#[async_trait]
impl AuthorizationCache for MockAuthorizationCache {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        let responses = self.responses.lock().unwrap();
        Ok(responses.get(cache_key).cloned())
    }

    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        _ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        let mut responses = self.responses.lock().unwrap();
        responses.insert(cache_key.to_string(), response.clone());
        Ok(())
    }

    async fn invalidate_principal(&self, _principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn invalidate_resource(&self, _resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock Authorization Logger for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationLogger {
    decisions_logged: Arc<Mutex<Vec<AuthorizationResponse>>>,
}

impl MockAuthorizationLogger {
    pub fn new() -> Self {
        Self {
            decisions_logged: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_logged_decisions(&self) -> Vec<AuthorizationResponse> {
        let logged = self.decisions_logged.lock().unwrap();
        logged.clone()
    }
}

#[async_trait]
impl AuthorizationLogger for MockAuthorizationLogger {
    async fn log_decision(
        &self,
        _request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()> {
        let mut logged = self.decisions_logged.lock().unwrap();
        logged.push(response.clone());
        Ok(())
    }

    async fn log_error(
        &self,
        _request: &AuthorizationRequest,
        _error: &crate::features::evaluate_permissions::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock Authorization Metrics for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationMetrics {
    decisions_recorded: Arc<Mutex<Vec<AuthorizationDecision>>>,
}

impl MockAuthorizationMetrics {
    pub fn new() -> Self {
        Self {
            decisions_recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_recorded_decisions(&self) -> Vec<AuthorizationDecision> {
        let recorded = self.decisions_recorded.lock().unwrap();
        recorded.clone()
    }
}

#[async_trait]
impl AuthorizationMetrics for MockAuthorizationMetrics {
    async fn record_decision(
        &self,
        decision: &AuthorizationDecision,
        _evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()> {
        let mut recorded = self.decisions_recorded.lock().unwrap();
        recorded.push(decision.clone());
        Ok(())
    }

    async fn record_error(&self, _error_type: &str) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn record_cache_hit(&self, _hit: bool) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock Entity for testing authorization
#[derive(Debug, Clone)]
pub struct MockHodeiEntity {
    hrn: Hrn,
    euid: EntityUid,
    attributes: HashMap<String, cedar_policy::RestrictedExpression>,
}

impl MockHodeiEntity {
    pub fn new(hrn: Hrn, euid: EntityUid) -> Self {
        Self {
            hrn,
            euid,
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(
        mut self,
        key: String,
        value: cedar_policy::RestrictedExpression,
    ) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

impl policies::domain::HodeiEntity for MockHodeiEntity {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn euid(&self) -> EntityUid {
        self.euid.clone()
    }

    fn attributes(&self) -> HashMap<String, cedar_policy::RestrictedExpression> {
        self.attributes.clone()
    }
}

/// Mock Entity Resolver for testing
#[derive(Debug, Default, Clone)]
pub struct MockEntityResolver {
    entities: Arc<Mutex<HashMap<String, MockHodeiEntity>>>,
}

impl MockEntityResolver {
    pub fn new() -> Self {
        Self {
            entities: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_entity(self, entity: MockHodeiEntity) -> Self {
        let hrn_str = entity.hrn.to_string();
        let mut entities = self.entities.lock().unwrap();
        entities.insert(hrn_str, entity);
        drop(entities);
        self
    }

    pub fn get_resolved_count(&self) -> usize {
        let entities = self.entities.lock().unwrap();
        entities.len()
    }
}

#[async_trait]
impl EntityResolverPort for MockEntityResolver {
    async fn resolve(
        &self,
        hrn: &Hrn,
    ) -> Result<Box<dyn policies::domain::HodeiEntity>, EntityResolverError> {
        let entities = self.entities.lock().unwrap();
        entities
            .get(&hrn.to_string())
            .map(|e| Box::new(e.clone()) as Box<dyn policies::domain::HodeiEntity>)
            .ok_or_else(|| EntityResolverError::NotFound(hrn.clone()))
    }

    async fn resolve_batch(
        &self,
        hrns: &[Hrn],
    ) -> Result<Vec<Box<dyn policies::domain::HodeiEntity>>, EntityResolverError> {
        let entities = self.entities.lock().unwrap();
        let mut resolved = Vec::new();
        let mut not_found = Vec::new();

        for hrn in hrns {
            if let Some(entity) = entities.get(&hrn.to_string()) {
                resolved.push(Box::new(entity.clone()) as Box<dyn policies::domain::HodeiEntity>);
            } else {
                not_found.push(hrn.clone());
            }
        }

        if !not_found.is_empty() {
            return Err(EntityResolverError::BatchResolutionFailed(not_found));
        }

        Ok(resolved)
    }
}

/// Mock SCP Repository for testing
#[derive(Debug, Clone)]
pub struct MockScpRepository;

/// Mock Org Repository for testing
#[derive(Debug, Clone)]
pub struct MockOrgRepository;

/// Helper functions for creating test data
pub mod test_helpers {
    use super::*;
    use crate::features::evaluate_permissions::dto::AuthorizationContext;

    /// Create a test HRN
    pub fn create_test_hrn(resource_type: &str, resource_id: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "us-east-1".to_string(),
            resource_type.to_string(),
            resource_id.to_string(),
        )
    }

    /// Create a test authorization request
    pub fn create_test_request(
        principal_hrn: Hrn,
        action: String,
        resource_hrn: Hrn,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            principal: principal_hrn,
            action,
            resource: resource_hrn,
            context: None,
        }
    }

    /// Create a test authorization request with context
    pub fn create_test_request_with_context(
        principal_hrn: Hrn,
        action: String,
        resource_hrn: Hrn,
        context: AuthorizationContext,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            principal: principal_hrn,
            action,
            resource: resource_hrn,
            context: Some(context),
        }
    }
}
