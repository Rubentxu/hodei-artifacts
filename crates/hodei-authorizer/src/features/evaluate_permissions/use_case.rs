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
use policies::shared::AuthorizationEngine;

// Importar casos de uso de otros crates (NO entidades internas)
use hodei_iam::GetEffectivePoliciesForPrincipalUseCase;
use hodei_iam::GetEffectivePoliciesQuery;
use hodei_organizations::GetEffectiveScpsQuery;

/// Use case for evaluating authorization permissions with multi-layer security
///
/// Esta implementación sigue el principio de responsabilidad única:
/// - NO gestiona políticas directamente
/// - USA casos de uso de otros crates para obtener políticas
/// - DELEGA la evaluación al AuthorizationEngine de policies
/// - Gestiona aspectos transversales: cache, logging, metrics
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
    // ✅ Casos de uso de otros crates (NO providers custom)
    iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,
    org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,

    // ✅ Motor de autorización del crate policies
    authorization_engine: Arc<AuthorizationEngine>,

    // ✅ Aspectos transversales
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
}

/// Trait para abstraer el caso de uso de SCPs efectivas
#[async_trait::async_trait]
pub trait GetEffectiveScpsPort: Send + Sync {
    async fn execute(
        &self,
        query: GetEffectiveScpsQuery,
    ) -> Result<cedar_policy::PolicySet, Box<dyn std::error::Error + Send + Sync>>;
}

impl<CACHE, LOGGER, METRICS> EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    /// Create a new instance of the use case
    pub fn new(
        iam_use_case: Arc<GetEffectivePoliciesForPrincipalUseCase>,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_use_case,
            org_use_case,
            authorization_engine,
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

        // Step 1: Get IAM policies using hodei-iam use case
        info!("Fetching IAM policies for principal");
        let iam_query = GetEffectivePoliciesQuery {
            principal_hrn: request.principal.to_string(),
        };

        let iam_response = self.iam_use_case.execute(iam_query).await.map_err(|e| {
            EvaluatePermissionsError::IamPolicyProviderError(format!(
                "Failed to get IAM policies: {}",
                e
            ))
        })?;

        info!(
            "Retrieved {} IAM policies for principal",
            iam_response.policy_count
        );

        // Step 2: Get SCPs using hodei-organizations use case (if available)
        let scp_policy_set = if let Some(ref org_use_case) = self.org_use_case {
            info!("Fetching effective SCPs for resource");
            let scp_query = GetEffectiveScpsQuery {
                resource_hrn: request.resource.to_string(),
            };

            let scp_response = org_use_case.execute(scp_query).await.map_err(|e| {
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
            info!("No organization use case configured, skipping SCPs");
            cedar_policy::PolicySet::new()
        };

        // Step 3: Combine PolicySets
        info!("Combining IAM policies and SCPs");
        let mut combined_policies = cedar_policy::PolicySet::new();

        // Add SCPs first (higher precedence in evaluation - deny overrides)
        for scp_policy in scp_policy_set.policies() {
            if let Err(e) = combined_policies.add(scp_policy.clone()) {
                warn!("Failed to add SCP policy: {}", e);
                // Continue with other policies even if one fails
            }
        }

        // Add IAM policies
        for iam_policy in iam_response.policies.policies() {
            if let Err(e) = combined_policies.add(iam_policy.clone()) {
                warn!("Failed to add IAM policy: {}", e);
                // Continue with other policies even if one fails
            }
        }

        info!(
            "Combined {} total policies, delegating evaluation to AuthorizationEngine",
            combined_policies.policies().count()
        );

        // Step 4: Delegate evaluation to policies crate's AuthorizationEngine
        let decision = self
            .evaluate_with_policy_set(&request, &combined_policies)
            .await?;

        info!(
            "Authorization evaluation completed: {:?}",
            decision.decision
        );
        Ok(decision)
    }

    /// Evaluate authorization by delegating to the policies crate's AuthorizationEngine
    /// This method will be enhanced once we have proper HodeiEntity implementations
    async fn evaluate_with_policy_set(
        &self,
        request: &AuthorizationRequest,
        policies: &cedar_policy::PolicySet,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        use cedar_policy::EntityUid;
        use std::str::FromStr;

        // If no policies, apply Principle of Least Privilege (implicit deny)
        if policies.is_empty() {
            info!("No policies found - applying Principle of Least Privilege (implicit deny)");
            return Ok(AuthorizationResponse::implicit_deny(
                "No policies matched - access denied by Principle of Least Privilege".to_string(),
            ));
        }

        // Convert request to Cedar format
        let principal = EntityUid::from_str(&request.principal.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid principal HRN: {}", e))
        })?;

        let action =
            EntityUid::from_str(&format!("Action::\"{}\"", request.action)).map_err(|e| {
                EvaluatePermissionsError::InvalidRequest(format!("Invalid action: {}", e))
            })?;

        let resource = EntityUid::from_str(&request.resource.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid resource HRN: {}", e))
        })?;

        let context = self.create_cedar_context(request)?;

        // Create authorization request for policies crate
        // Note: Using empty entities vector for now - will be enhanced with proper entity resolution
        let auth_request = policies::shared::AuthorizationRequest {
            principal: &MockHodeiEntity {
                euid: principal.clone(),
                mock_hrn: policies::shared::Hrn::from_string("hrn:hodei:iam::principal/mock")
                    .unwrap(),
            },
            action: action.clone(),
            resource: &MockHodeiEntity {
                euid: resource.clone(),
                mock_hrn: policies::shared::Hrn::from_string("hrn:hodei:resource::mock/resource")
                    .unwrap(),
            },
            context,
            entities: vec![], // Will be populated with actual entities later
        };

        // Delegate to policies crate's AuthorizationEngine with the combined PolicySet
        info!("Delegating evaluation to policies::AuthorizationEngine");
        let response = self
            .authorization_engine
            .is_authorized_with_policy_set(&auth_request, policies);

        // Convert Cedar response to our DTO
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

    /// Create Cedar context from request context
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
            if let Some(ref request_time) = request_context.request_time {
                if let Ok(formatted) = request_time.format(&Rfc3339) {
                    context_data.insert(
                        "request_time".to_string(),
                        serde_json::Value::String(formatted),
                    );
                }
            }

            // Add additional context
            for (key, value) in &request_context.additional_context {
                context_data.insert(key.clone(), value.clone());
            }
        }

        let context_json = serde_json::Value::Object(context_data);
        cedar_policy::Context::from_json_value(context_json, None).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid context: {}", e))
        })
    }

    /// Generate cache key for authorization request
    fn generate_cache_key(&self, request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }
}

/// Temporary mock implementation of HodeiEntity for authorization requests
/// This will be replaced with proper entity implementations from hodei-iam and hodei-organizations
struct MockHodeiEntity {
    euid: cedar_policy::EntityUid,
    mock_hrn: policies::shared::Hrn,
}

impl policies::domain::HodeiEntity for MockHodeiEntity {
    fn hrn(&self) -> &policies::shared::Hrn {
        &self.mock_hrn
    }
    fn euid(&self) -> cedar_policy::EntityUid {
        self.euid.clone()
    }

    fn attributes(&self) -> std::collections::HashMap<String, cedar_policy::RestrictedExpression> {
        std::collections::HashMap::new()
    }

    fn parents(&self) -> Vec<cedar_policy::EntityUid> {
        vec![]
    }
}
