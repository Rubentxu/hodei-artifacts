use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::sync::Arc;

use crate::features::evaluate_permissions::dto::{AuthorizationRequest, AuthorizationResponse};
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use policies::shared::domain::hrn::Hrn;

/// Trait for providing IAM policies
#[async_trait]
pub trait IamPolicyProvider: Send + Sync {
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet>;
}

#[async_trait]
impl<T: IamPolicyProvider> IamPolicyProvider for Arc<T> {
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        (**self).get_identity_policies_for(principal_hrn).await
    }
}

/// Trait for providing organization boundary policies (SCPs)
#[async_trait]
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(
        &self,
        entity_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet>;
}

#[async_trait]
impl<T: OrganizationBoundaryProvider> OrganizationBoundaryProvider for Arc<T> {
    async fn get_effective_scps_for(
        &self,
        entity_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        (**self).get_effective_scps_for(entity_hrn).await
    }
}

/// Trait for caching authorization decisions
#[async_trait]
pub trait AuthorizationCache: Send + Sync {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>>;
    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()>;
    async fn invalidate_principal(&self, principal_hrn: &Hrn) -> EvaluatePermissionsResult<()>;
    async fn invalidate_resource(&self, resource_hrn: &Hrn) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationCache> AuthorizationCache for Arc<T> {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        (**self).get(cache_key).await
    }

    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        (**self).put(cache_key, response, ttl).await
    }

    async fn invalidate_principal(&self, principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        (**self).invalidate_principal(principal_hrn).await
    }

    async fn invalidate_resource(&self, resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        (**self).invalidate_resource(resource_hrn).await
    }
}

/// Trait for logging authorization decisions and errors
#[async_trait]
pub trait AuthorizationLogger: Send + Sync {
    async fn log_decision(
        &self,
        request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()>;
    async fn log_error(
        &self,
        request: &AuthorizationRequest,
        error: &super::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationLogger> AuthorizationLogger for Arc<T> {
    async fn log_decision(
        &self,
        request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()> {
        (**self).log_decision(request, response).await
    }

    async fn log_error(
        &self,
        request: &AuthorizationRequest,
        error: &super::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()> {
        (**self).log_error(request, error).await
    }
}

/// Trait for recording authorization metrics
#[async_trait]
pub trait AuthorizationMetrics: Send + Sync {
    async fn record_decision(
        &self,
        decision: &super::dto::AuthorizationDecision,
        evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()>;
    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()>;
    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationMetrics> AuthorizationMetrics for Arc<T> {
    async fn record_decision(
        &self,
        decision: &super::dto::AuthorizationDecision,
        evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()> {
        (**self).record_decision(decision, evaluation_time_ms).await
    }

    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()> {
        (**self).record_error(error_type).await
    }

    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()> {
        (**self).record_cache_hit(hit).await
    }
}

/// Errors related to authorization ports
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("IAM policy provider error: {0}")]
    IamPolicyProvider(String),
    #[error("Organization boundary provider error: {0}")]
    OrganizationBoundaryProvider(String),
}
