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
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics, EntityResolverPort,
};
use policies::shared::AuthorizationEngine;
use kernel::{
    // Cross-context IAM + Organizations ports re-exported from shared crate (kernel)
    EffectivePoliciesQuery,
    EffectivePoliciesQueryPort,
    GetEffectiveScpsPort,
    GetEffectiveScpsQuery,
};

/// Use case for evaluating authorization permissions with multi-layer security
///
/// Esta implementación sigue el principio de responsabilidad única:
/// - NO gestiona políticas directamente
/// - Obtiene políticas IAM y SCP vía puertos cross-context (shared kernel)
/// - DELEGA la evaluación al AuthorizationEngine de policies
/// - Gestiona aspectos transversales: cache, logging, metrics
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
    // Puertos cross-context (no dependemos de casos de uso concretos de otros crates)
    iam_port: Arc<dyn EffectivePoliciesQueryPort>,
    org_port: Option<Arc<dyn GetEffectiveScpsPort>>,

    // Motor de autorización del crate policies
    authorization_engine: Arc<AuthorizationEngine>,

    // Entity resolver para obtener entidades reales
    entity_resolver: Arc<dyn EntityResolverPort>,

    // Aspectos transversales
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
    /// Create a new instance of the use case using cross-context ports
    pub fn new(
        iam_port: Arc<dyn EffectivePoliciesQueryPort>,
        org_port: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        entity_resolver: Arc<dyn EntityResolverPort>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_port,
            org_port,
            authorization_engine,
            entity_resolver,
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

    /// Core authorization evaluation logic - orchestrates policy collection and delegates to AuthorizationEngine
    async fn evaluate_authorization(
        &self,
        request: &AuthorizationRequest,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        info!("Starting multi-layer authorization evaluation (orchestration)");

        // Step 1: Get IAM policies via cross-context port
        info!("Fetching IAM policies for principal");
        let iam_query = EffectivePoliciesQuery {
            principal_hrn: request.principal.to_string(),
        };

        let iam_response = self
            .iam_port
            .get_effective_policies(iam_query)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::IamPolicyProviderError(format!(
                    "Failed to get IAM policies: {}",
                    e
                ))
            })?;

        info!(
            "Retrieved {} IAM policies for principal",
            iam_response.policy_count
        );

        // Step 2: Get SCPs if organizations port available
        let scp_policy_set = if let Some(ref org_port) = self.org_port {
            info!("Fetching effective SCPs for resource");
            let scp_query = GetEffectiveScpsQuery {
                resource_hrn: request.resource.to_string(),
            };

            let scp_response = org_port.get_effective_scps(scp_query).await.map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to get SCPs: {}",
                    e
                ))
            })?;

            info!(
                "Retrieved {} SCPs for resource",
                scp_response.policies().count()
            );
            scp_response
        } else {
            info!("No organization port configured, skipping SCPs");
            cedar_policy::PolicySet::new()
        };

        // Step 3: Combine PolicySets
        info!("Combining IAM policies and SCPs");
        let mut combined_policies = cedar_policy::PolicySet::new();

        // Add SCPs first (higher precedence in evaluation - deny overrides)
        for scp_policy in scp_policy_set.policies() {
            if let Err(e) = combined_policies.add(scp_policy.clone()) {
                warn!("Failed to add SCP policy: {}", e);
            }
        }

        // Add IAM policies
        for iam_policy in iam_response.policies.policies() {
            if let Err(e) = combined_policies.add(iam_policy.clone()) {
                warn!("Failed to add IAM policy: {}", e);
            }
        }

        info!(
            "Combined {} total policies, delegating evaluation to AuthorizationEngine",
            combined_policies.policies().count()
        );

        // Step 4: Delegate evaluation to policies crate's AuthorizationEngine
        let decision = self
            .evaluate_with_policy_set(request, &combined_policies)
            .await?;

        info!(
            "Authorization evaluation completed: {:?}",
            decision.decision
        );
        Ok(decision)
    }

    /// Evaluate authorization by delegating to the policies crate's AuthorizationEngine
    async fn evaluate_with_policy_set(
        &self,
        request: &AuthorizationRequest,
        policies: &cedar_policy::PolicySet,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        use cedar_policy::EntityUid;
        use std::str::FromStr;

        if policies.is_empty() {
            info!("No policies found - applying Principle of Least Privilege (implicit deny)");
            return Ok(AuthorizationResponse::implicit_deny(
                "No policies matched - access denied by Principle of Least Privilege".to_string(),
            ));
        }

        let _principal = EntityUid::from_str(&request.principal.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid principal HRN: {}", e))
        })?;

        let action =
            EntityUid::from_str(&format!("Action::\"{}\"", request.action)).map_err(|e| {
                EvaluatePermissionsError::InvalidRequest(format!("Invalid action: {}", e))
            })?;

        let _resource = EntityUid::from_str(&request.resource.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid resource HRN: {}", e))
        })?;

        let context = self.create_cedar_context(request)?;

        let principal_entity = self
            .entity_resolver
            .resolve(&request.principal)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::EntityResolutionError(format!(
                    "Failed to resolve principal: {}",
                    e
                ))
            })?;

        let resource_entity = self
            .entity_resolver
            .resolve(&request.resource)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::EntityResolutionError(format!(
                    "Failed to resolve resource: {}",
                    e
                ))
            })?;

        let auth_request = policies::shared::AuthorizationRequest {
            principal: principal_entity.as_ref(),
            action: action.clone(),
            resource: resource_entity.as_ref(),
            context,
            entities: vec![],
        };

        info!("Delegating evaluation to policies::AuthorizationEngine");
        let response = self
            .authorization_engine
            .is_authorized_with_policy_set(&auth_request, policies);

        let (decision, determining_policies, explicit, reason) = match response.decision() {
            cedar_policy::Decision::Deny => {
                let policies: Vec<String> = response
                    .diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Deny,
                    policies,
                    true,
                    "Access explicitly denied by policy".to_string(),
                )
            }
            cedar_policy::Decision::Allow => {
                let policies: Vec<String> = response
                    .diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Allow,
                    policies,
                    true,
                    "Access explicitly allowed by policy".to_string(),
                )
            }
        };

        Ok(AuthorizationResponse {
            decision,
            determining_policies,
            reason,
            explicit,
        })
    }

    fn create_cedar_context(
        &self,
        request: &AuthorizationRequest,
    ) -> EvaluatePermissionsResult<cedar_policy::Context> {
        use time::format_description::well_known::Rfc3339;

        let mut context_data = serde_json::Map::new();

        if let Some(ref request_context) = request.context {
            if let Some(ref source_ip) = request_context.source_ip {
                context_data.insert(
                    "source_ip".to_string(),
                    serde_json::Value::String(source_ip.clone()),
                );
            }
            if let Some(ref user_agent) = request_context.user_agent {
                context_data.insert(
                    "user_agent".to_string(),
                    serde_json::Value::String(user_agent.clone()),
                );
            }
            if let Some(ref request_time) = request_context.request_time
                && let Ok(formatted) = request_time.format(&Rfc3339)
            {
                context_data.insert(
                    "request_time".to_string(),
                    serde_json::Value::String(formatted),
                );
            }
            for (key, value) in &request_context.additional_context {
                context_data.insert(key.clone(), value.clone());
            }
        }

        let context_json = serde_json::Value::Object(context_data);
        cedar_policy::Context::from_json_value(context_json, None).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid context: {}", e))
        })
    }

    fn generate_cache_key(&self, request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }
}
