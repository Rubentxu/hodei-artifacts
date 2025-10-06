use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::features::evaluate_permissions::dto::{
    AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
};
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use ::kernel::Hrn;
use kernel::application::ports::authorization::{
    AuthorizationError, EvaluationDecision, EvaluationRequest, IamPolicyEvaluator, ScpEvaluator,
};

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

// ============================================================================
// Mock Evaluators for New Architecture
// ============================================================================

/// Mock SCP Evaluator that can be configured to allow or deny
#[derive(Debug, Clone)]
pub struct MockScpEvaluator {
    should_deny: bool,
}

impl Default for MockScpEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl MockScpEvaluator {
    pub fn new() -> Self {
        Self { should_deny: false }
    }

    pub fn with_deny() -> Self {
        Self { should_deny: true }
    }
}

#[async_trait]
impl ScpEvaluator for MockScpEvaluator {
    async fn evaluate_scps(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        Ok(EvaluationDecision {
            principal_hrn: request.principal_hrn,
            action_name: request.action_name,
            resource_hrn: request.resource_hrn,
            decision: !self.should_deny,
            reason: if self.should_deny {
                "Denied by SCP mock".to_string()
            } else {
                "Allowed by SCP mock".to_string()
            },
        })
    }
}

/// Mock IAM Policy Evaluator that can be configured to allow or deny
#[derive(Debug, Clone)]
pub struct MockIamPolicyEvaluator {
    should_deny: bool,
}

impl Default for MockIamPolicyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl MockIamPolicyEvaluator {
    pub fn new() -> Self {
        Self { should_deny: false }
    }

    pub fn with_deny() -> Self {
        Self { should_deny: true }
    }
}

#[async_trait]
impl IamPolicyEvaluator for MockIamPolicyEvaluator {
    async fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError> {
        Ok(EvaluationDecision {
            principal_hrn: request.principal_hrn,
            action_name: request.action_name,
            resource_hrn: request.resource_hrn,
            decision: !self.should_deny,
            reason: if self.should_deny {
                "Denied by IAM mock".to_string()
            } else {
                "Allowed by IAM mock".to_string()
            },
        })
    }
}

/// Mock Entity Resolver for testing (simplified placeholder)
#[derive(Debug, Default, Clone)]
pub struct MockEntityResolver;

impl MockEntityResolver {
    pub fn new() -> Self {
        Self
    }
}
