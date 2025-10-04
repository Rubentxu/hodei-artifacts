use std::time::Instant;
use tracing::{info, warn, instrument};
use cedar_policy::{PolicySet, Entities, Authorizer, Request, Context, EntityUid, Schema, RequestValidationError, ContextJsonError};
use std::str::FromStr;
use time::format_description::well_known::Rfc3339;

use crate::features::evaluate_permissions::dto::{AuthorizationRequest, AuthorizationResponse, AuthorizationDecision};
use crate::features::evaluate_permissions::ports::{
    IamPolicyProvider, OrganizationBoundaryProvider, AuthorizationCache, 
    AuthorizationLogger, AuthorizationMetrics, EntityResolver
};
use crate::features::evaluate_permissions::error::{EvaluatePermissionsError, EvaluatePermissionsResult};

/// Use case for evaluating authorization permissions with multi-layer security
pub struct EvaluatePermissionsUseCase<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> {
    iam_provider: IAM,
    org_provider: ORG,
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
    entity_resolver: RESOLVER,
    cedar_authorizer: Authorizer,
}

impl<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> 
    EvaluatePermissionsUseCase<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
where
    IAM: IamPolicyProvider,
    ORG: OrganizationBoundaryProvider,
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
    RESOLVER: EntityResolver,
{
    /// Create a new instance of the use case
    pub fn new(
        iam_provider: IAM,
        org_provider: ORG,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
        entity_resolver: RESOLVER,
    ) -> Self {
        Self {
            iam_provider,
            org_provider,
            cache,
            logger,
            metrics,
            entity_resolver,
            cedar_authorizer: Authorizer::new(),
        }
    }

    /// Evaluate authorization request with multi-layer security
    #[instrument(skip(self), fields(principal = %request.principal, resource = %request.resource, action = %request.action))]
    pub async fn execute(&self, request: AuthorizationRequest) -> EvaluatePermissionsResult<AuthorizationResponse> {
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
                self.metrics.record_decision(&response.decision, evaluation_time_ms).await?;
            }
            Err(error) => {
                self.logger.log_error(&request, error).await?;
                self.metrics.record_error(std::any::type_name_of_val(error)).await?;
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

    /// Core authorization evaluation logic
    async fn evaluate_authorization(&self, request: &AuthorizationRequest) -> EvaluatePermissionsResult<AuthorizationResponse> {
        info!("Starting multi-layer authorization evaluation");

        // Step 1: Get effective SCPs for the resource (highest precedence)
        let effective_scps = self.org_provider.get_effective_scps_for(&request.resource).await?;
        info!("Retrieved {} effective SCPs for resource", effective_scps.policies().count());

        // Step 2: Evaluate against SCPs first (deny overrides)
        if !effective_scps.is_empty() {
            let cedar_request = self.convert_to_cedar_request(request)?;
            let entities = self.create_entities_data(request).await?;

            let response = self.cedar_authorizer.is_authorized(&cedar_request, &effective_scps, &entities);
            if response.decision() == cedar_policy::Decision::Deny {
                let auth_response = AuthorizationResponse::deny(
                    vec!["SCP Policy".to_string()],
                    "Access denied by Service Control Policy".to_string(),
                );
                info!("Access denied by SCP policy - evaluation terminated");
                return Ok(auth_response);
            }
            // If Allow or no matching policies, continue to IAM evaluation
            info!("SCP evaluation passed or no matching SCPs - continuing to IAM evaluation");
        } else {
            info!("No SCPs found for resource - continuing to IAM evaluation");
        }

        // Step 3: Get IAM policies (only if SCPs didn't deny)
        let iam_policies = self.iam_provider.get_identity_policies_for(&request.principal).await?;
        info!("Retrieved IAM policies for principal");

        // Step 4: Evaluate against IAM policies
        let iam_decision = self.evaluate_against_iam_policies(request, &iam_policies).await?;

        info!("Authorization evaluation completed: {:?}", iam_decision.decision);
        Ok(iam_decision)
    }


    /// Evaluate request against IAM policies following AWS authorization model
    async fn evaluate_against_iam_policies(
        &self,
        request: &AuthorizationRequest,
        iam_policies: &PolicySet,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        info!("Evaluating request against {} IAM policies", iam_policies.policies().count());
        
        // If no IAM policies apply, apply Principle of Least Privilege (implicit deny)
        if iam_policies.is_empty() {
            info!("No IAM policies found for principal - applying Principle of Least Privilege (implicit deny)");
            return Ok(AuthorizationResponse::implicit_deny(
                "No IAM policies matched - access denied by Principle of Least Privilege".to_string()
            ));
        }

        let cedar_request = self.convert_to_cedar_request(request)?;
        let entities = self.create_entities_data(request).await?;

        let response = self.cedar_authorizer.is_authorized(&cedar_request, iam_policies, &entities);
        
        // Analyze Cedar response and determine the final decision
        let (decision, determining_policies, explicit, reason) = match response.decision() {
            cedar_policy::Decision::Deny => {
                // Explicit deny from IAM policy
                let policies: Vec<String> = response.diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Deny,
                    policies,
                    true,
                    "Access explicitly denied by IAM policy".to_string()
                )
            }
            cedar_policy::Decision::Allow => {
                // Explicit allow from IAM policy
                let policies: Vec<String> = response.diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Allow,
                    policies,
                    true,
                    "Access explicitly allowed by IAM policy".to_string()
                )
            }
        };

        info!("IAM policy evaluation completed: {:?} (explicit: {})", decision, explicit);

        Ok(AuthorizationResponse {
            decision,
            determining_policies,
            reason,
            explicit,
        })
    }


    /// Convert authorization request to Cedar request format
    fn convert_to_cedar_request(&self, request: &AuthorizationRequest) -> EvaluatePermissionsResult<Request> {
        let principal = EntityUid::from_str(&request.principal.to_string())
            .map_err(|e| EvaluatePermissionsError::InvalidRequest(format!("Invalid principal HRN: {}", e)))?;

        let action = EntityUid::from_str(&format!("Action::\"{}\"", request.action))
            .map_err(|e| EvaluatePermissionsError::InvalidRequest(format!("Invalid action: {}", e)))?;

        let resource = EntityUid::from_str(&request.resource.to_string())
            .map_err(|e| EvaluatePermissionsError::InvalidRequest(format!("Invalid resource HRN: {}", e)))?;

        let context = self.create_cedar_context(request)?;

        Request::new(
            principal,
            action,
            resource,
            context,
            None::<&Schema>,
        ).map_err(|e: RequestValidationError| EvaluatePermissionsError::InvalidRequest(e.to_string()))
    }

    /// Create Cedar context from request context
    fn create_cedar_context(&self, request: &AuthorizationRequest) -> EvaluatePermissionsResult<Context> {
        let mut context_data = serde_json::Map::new();

        if let Some(ref request_context) = request.context {
            if let Some(ref source_ip) = request_context.source_ip {
                context_data.insert("source_ip".to_string(), serde_json::Value::String(source_ip.clone()));
            }
            if let Some(ref user_agent) = request_context.user_agent {
                context_data.insert("user_agent".to_string(), serde_json::Value::String(user_agent.clone()));
            }
            if let Some(ref request_time) = request_context.request_time {
                context_data.insert("request_time".to_string(), 
                    serde_json::Value::String(request_time.format(&Rfc3339).unwrap()));
            }

            // Add additional context
            for (key, value) in &request_context.additional_context {
                context_data.insert(key.clone(), value.clone());
            }
        }

        let context_json = serde_json::Value::Object(context_data);
        Context::from_json_value(context_json, None)
            .map_err(|e| EvaluatePermissionsError::InvalidRequest(format!("Invalid context: {}", e)))
    }

    /// Create entities data for Cedar evaluation
    async fn create_entities_data(&self, request: &AuthorizationRequest) -> EvaluatePermissionsResult<Entities> {
        // Resolve principal entity
        let principal_entity = self.entity_resolver.resolve_entity(&request.principal).await?;

        // Resolve resource entity
        let resource_entity = self.entity_resolver.resolve_entity(&request.resource).await?;

        Entities::from_entities(vec![principal_entity, resource_entity], None)
            .map_err(|e| EvaluatePermissionsError::InternalError(e.to_string()))
    }


    /// Generate cache key for authorization request
    fn generate_cache_key(&self, request: &AuthorizationRequest) -> String {
        format!("auth:{}:{}:{}", request.principal, request.action, request.resource)
    }
}

// Required trait implementations
impl From<cedar_policy::ParseErrors> for EvaluatePermissionsError {
    fn from(err: cedar_policy::ParseErrors) -> Self {
        EvaluatePermissionsError::InvalidRequest(format!("Entity UID parse error: {}", err))
    }
}

impl From<ContextJsonError> for EvaluatePermissionsError {
    fn from(err: ContextJsonError) -> Self {
        EvaluatePermissionsError::InvalidRequest(format!("Context error: {}", err))
    }
}
