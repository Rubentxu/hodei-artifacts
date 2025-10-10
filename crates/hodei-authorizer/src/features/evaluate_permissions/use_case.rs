use std::sync::Arc;
use std::time::Instant;
use tracing::{info, instrument, warn};

use crate::features::evaluate_permissions::dto::{
    AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
};
use crate::features::evaluate_permissions::error::{
    EvaluatePermissionsError, EvaluatePermissionsResult,
};
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use kernel::application::ports::authorization::{
    EvaluationRequest, IamPolicyEvaluator, ScpEvaluator,
};

/// Use case for evaluating authorization permissions with multi-layer security
///
/// This implementation follows the Single Responsibility Principle:
/// - It does NOT manage policies directly
/// - It delegates IAM policy evaluation to IamPolicyEvaluator trait
/// - It delegates SCP evaluation to ScpEvaluator trait
/// - It manages cross-cutting concerns: cache, logging, metrics
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
    // Cross-context evaluators (we don't depend on concrete use cases from other crates)
    iam_evaluator: Arc<dyn IamPolicyEvaluator>,
    org_evaluator: Arc<dyn ScpEvaluator>,

    // Cross-cutting concerns
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
}

impl<CACHE, LOGGER, METRICS> EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    /// Create a new instance of the use case using cross-context evaluators
    pub fn new(
        iam_evaluator: Arc<dyn IamPolicyEvaluator>,
        org_evaluator: Arc<dyn ScpEvaluator>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_evaluator,
            org_evaluator,
            cache,
            logger,
            metrics,
        }
    }

    /// Evaluate authorization request with multi-layer security
    #[instrument(skip(self), fields(principal = %request.principal, resource = %request.resource, action = %request.action))]
    pub async fn execute(
        &self,
        request: AuthorizationRequest,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        let start_time = Instant::now();

        // Generate cache key and check cache first
        let cache_key = self.generate_cache_key(&request);
        if let Some(ref cache) = self.cache {
            if let Ok(Some(cached_response)) = cache.get(&cache_key).await {
                info!("Authorization decision served from cache");
                self.metrics.record_cache_hit(true).await?;
                return Ok(cached_response);
            }
            self.metrics.record_cache_hit(false).await?;
        }

        // Execute the evaluation
        let result = self.evaluate_authorization(&request).await;
        let evaluation_time_ms = start_time.elapsed().as_millis() as u64;

        // Log and record metrics
        match &result {
            Ok(response) => {
                self.logger.log_decision(&request, response).await?;
                self.metrics
                    .record_decision(&response.decision, evaluation_time_ms)
                    .await?;
            }
            Err(error) => {
                self.logger.log_error(&request, error).await?;
                self.metrics
                    .record_error(std::any::type_name_of_val(error))
                    .await?;
            }
        }

        // Cache the result if successful
        if let (Ok(response), Some(cache)) = (&result, &self.cache) {
            let ttl = std::time::Duration::from_secs(300); // 5 minutes cache
            if let Err(cache_error) = cache.put(&cache_key, response, ttl).await {
                warn!("Failed to cache authorization decision: {}", cache_error);
            }
        }

        result
    }

    /// Core authorization evaluation logic - orchestrates policy evaluation via delegated traits
    async fn evaluate_authorization(
        &self,
        request: &AuthorizationRequest,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        info!("Starting multi-layer authorization evaluation (orchestration)");

        // Convert to kernel's EvaluationRequest (zero-copy)
        let eval_request = EvaluationRequest {
            principal_hrn: request.principal.clone(),
            action_name: request.action.clone(),
            resource_hrn: request.resource.clone(),
        };
        };

        // Step 1: Evaluate SCPs first (higher precedence in evaluation - deny overrides)
        info!("Evaluating SCPs for resource");
        let scp_decision = self
            .org_evaluator
            .evaluate_scps(eval_request.clone())
            .await
            .map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to evaluate SCPs: {}",
                    e
                ))
            })?;

        // If SCP explicitly denies, return deny decision immediately
        if !scp_decision.decision {
            info!("Access denied by SCP policy");
            return Ok(AuthorizationResponse {
                decision: AuthorizationDecision::Deny,
                determining_policies: vec![],
                reason: scp_decision.reason,
                explicit: true,
            });
        }

        // Step 2: Evaluate IAM policies
        info!("Evaluating IAM policies for principal");
        let iam_decision = self
            .iam_evaluator
            .evaluate_iam_policies(eval_request)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::IamPolicyProviderError(format!(
                    "Failed to evaluate IAM policies: {}",
                    e
                ))
            })?;

        info!(
            "Authorization evaluation completed: {:?}",
            iam_decision.decision
        );

        Ok(AuthorizationResponse {
            decision: if iam_decision.decision {
                AuthorizationDecision::Allow
            } else {
                AuthorizationDecision::Deny
            },
            determining_policies: vec![],
            reason: iam_decision.reason,
            explicit: true,
        })
    }

    fn generate_cache_key(&self, request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }
}
