//! Use case for evaluating Cedar policies
//!
//! This use case provides a generic policy evaluation service using Cedar's Authorizer.
//! It is domain-agnostic and can be used by any bounded context (IAM, Organizations, etc.)
//! to evaluate Cedar policies.

use super::dto::{
    Decision, EntityDefinition, EvaluatePoliciesRequest, EvaluatePoliciesResponse,
    EvaluationDiagnostics,
};
use cedar_policy::{
    Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy,
    PolicySet, Request, RestrictedExpression,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Error types for policy evaluation
#[derive(Debug, thiserror::Error)]
pub enum EvaluatePoliciesError {
    #[error("Policy parse error: {0}")]
    PolicyParseError(String),

    #[error("Entity UID parse error: {0}")]
    EntityUidParseError(String),

    #[error("Request build error: {0}")]
    RequestBuildError(String),

    #[error("Entity creation error: {0}")]
    EntityCreationError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

/// Use case for evaluating Cedar policies
///
/// This is a stateless service that evaluates Cedar policies using the Cedar Authorizer.
/// It follows the Single Responsibility Principle and only handles policy evaluation logic.
pub struct EvaluatePoliciesUseCase;

impl EvaluatePoliciesUseCase {
    /// Create a new instance of the use case
    pub fn new() -> Self {
        Self
    }

    /// Execute policy evaluation
    ///
    /// # Arguments
    /// * `request` - The evaluation request containing policies, principal, action, resource
    ///
    /// # Returns
    /// The evaluation response with decision and diagnostics
    pub async fn execute(
        &self,
        request: EvaluatePoliciesRequest,
    ) -> Result<EvaluatePoliciesResponse, EvaluatePoliciesError> {
        let start = Instant::now();

        debug!(
            principal = %request.principal,
            action = %request.action,
            resource = %request.resource,
            policy_count = request.policies.len(),
            "Starting policy evaluation"
        );

        // Validate request
        if request.policies.is_empty() {
            return Err(EvaluatePoliciesError::InvalidRequest(
                "At least one policy is required".to_string(),
            ));
        }

        // Build PolicySet
        let (policy_set, mut diagnostics) = self.build_policy_set(&request.policies)?;

        // Build Cedar request
        let cedar_request = self.build_cedar_request(&request)?;

        // Build entities
        let entities = self.build_entities(&request.entities)?;

        // Evaluate with Cedar Authorizer
        let authorizer = Authorizer::new();
        let response = authorizer.is_authorized(&cedar_request, &policy_set, &entities);

        // Extract decision
        let decision = match response.decision() {
            CedarDecision::Allow => Decision::Allow,
            CedarDecision::Deny => Decision::Deny,
        };

        // Collect diagnostics from Cedar response
        let reason_policies: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|pid| pid.to_string())
            .collect();

        let error_policies: Vec<String> = response
            .diagnostics()
            .errors()
            .map(|err| err.to_string())
            .collect();

        if !error_policies.is_empty() {
            diagnostics.errored_policies.extend(error_policies.clone());
            diagnostics.errors.extend(error_policies);
        }

        if !reason_policies.is_empty() {
            diagnostics.satisfied_policies = reason_policies.clone();
        }

        let reason = if decision == Decision::Allow {
            if reason_policies.is_empty() {
                "Allowed by default (no explicit deny)".to_string()
            } else {
                format!("Allowed by policies: {}", reason_policies.join(", "))
            }
        } else if diagnostics.errors.is_empty() {
            "Denied (no matching allow policy)".to_string()
        } else {
            format!("Denied due to errors: {}", diagnostics.errors.join("; "))
        };

        let evaluation_time = start.elapsed().as_micros() as u64;

        info!(
            decision = ?decision,
            evaluation_time_us = evaluation_time,
            satisfied_policies = reason_policies.len(),
            errors = diagnostics.errors.len(),
            "Policy evaluation completed"
        );

        Ok(EvaluatePoliciesResponse {
            decision,
            reason,
            diagnostics: Some(diagnostics),
            evaluation_time_us: evaluation_time,
        })
    }

    /// Build a PolicySet from policy strings
    fn build_policy_set(
        &self,
        policies: &[String],
    ) -> Result<(PolicySet, EvaluationDiagnostics), EvaluatePoliciesError> {
        let mut policy_set = PolicySet::new();
        let mut diagnostics = EvaluationDiagnostics::default();

        for (idx, policy_str) in policies.iter().enumerate() {
            match policy_str.parse::<Policy>() {
                Ok(policy) => {
                    if let Err(e) = policy_set.add(policy) {
                        let error_msg = format!("Failed to add policy[{}]: {}", idx, e);
                        warn!(policy_index = idx, error = %e, "Failed to add policy to set");
                        diagnostics.errors.push(error_msg);
                    }
                }
                Err(e) => {
                    let error_msg = format!("Failed to parse policy[{}]: {}", idx, e);
                    warn!(policy_index = idx, error = %e, "Failed to parse policy");
                    diagnostics.errors.push(error_msg.clone());
                    return Err(EvaluatePoliciesError::PolicyParseError(error_msg));
                }
            }
        }

        Ok((policy_set, diagnostics))
    }

    /// Build a Cedar Request from the evaluation request
    fn build_cedar_request(
        &self,
        request: &EvaluatePoliciesRequest,
    ) -> Result<Request, EvaluatePoliciesError> {
        // Parse principal
        let principal = EntityUid::from_str(&request.principal).map_err(|e| {
            EvaluatePoliciesError::EntityUidParseError(format!(
                "Invalid principal '{}': {}",
                request.principal, e
            ))
        })?;

        // Parse action
        let action = EntityUid::from_str(&request.action).map_err(|e| {
            EvaluatePoliciesError::EntityUidParseError(format!(
                "Invalid action '{}': {}",
                request.action, e
            ))
        })?;

        // Parse resource
        let resource = EntityUid::from_str(&request.resource).map_err(|e| {
            EvaluatePoliciesError::EntityUidParseError(format!(
                "Invalid resource '{}': {}",
                request.resource, e
            ))
        })?;

        // Build context
        let context = self.build_context(request.context.as_ref());

        // Build Cedar Request
        Request::new(principal, action, resource, context, None).map_err(|e| {
            EvaluatePoliciesError::RequestBuildError(format!("Failed to build request: {}", e))
        })
    }

    /// Build context from optional context map
    fn build_context(
        &self,
        ctx: Option<&HashMap<String, serde_json::Value>>,
    ) -> Context {
        let mut map: HashMap<String, RestrictedExpression> = HashMap::new();

        if let Some(context_map) = ctx {
            for (key, value) in context_map {
                if let Some(expr) = json_to_expr(value) {
                    map.insert(key.clone(), expr);
                } else {
                    warn!(key = %key, "Failed to convert context value to Cedar expression");
                }
            }
        }

        Context::from_pairs(map).unwrap_or_else(|e| {
            warn!(error = %e, "Failed to create context, using empty context");
            Context::empty()
        })
    }

    /// Build Entities from entity definitions
    fn build_entities(
        &self,
        definitions: &[EntityDefinition],
    ) -> Result<Entities, EvaluatePoliciesError> {
        if definitions.is_empty() {
            return Ok(Entities::empty());
        }

        let mut entities = Vec::with_capacity(definitions.len());

        for (idx, def) in definitions.iter().enumerate() {
            // Parse entity UID
            let uid = EntityUid::from_str(&def.uid).map_err(|e| {
                EvaluatePoliciesError::EntityUidParseError(format!(
                    "Invalid entity UID '{}' at index {}: {}",
                    def.uid, idx, e
                ))
            })?;

            // Convert attributes
            let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
            for (key, value) in &def.attributes {
                if let Some(expr) = json_to_expr(value) {
                    attrs.insert(key.clone(), expr);
                } else {
                    warn!(
                        entity_uid = %def.uid,
                        attribute = %key,
                        "Failed to convert attribute value"
                    );
                }
            }

            // Parse parent UIDs
            let mut parents: HashSet<EntityUid> = HashSet::new();
            for parent_str in &def.parents {
                let parent_uid = EntityUid::from_str(parent_str).map_err(|e| {
                    EvaluatePoliciesError::EntityUidParseError(format!(
                        "Invalid parent UID '{}' for entity '{}': {}",
                        parent_str, def.uid, e
                    ))
                })?;
                parents.insert(parent_uid);
            }

            // Create entity
            let entity = Entity::new(uid, attrs, parents).map_err(|e| {
                EvaluatePoliciesError::EntityCreationError(format!(
                    "Failed to create entity '{}': {}",
                    def.uid, e
                ))
            })?;

            entities.push(entity);
        }

        Entities::from_entities(entities, None).map_err(|e| {
            EvaluatePoliciesError::EntityCreationError(format!("Failed to create Entities: {}", e))
        })
    }
}

impl Default for EvaluatePoliciesUseCase {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to convert JSON values to Cedar RestrictedExpression
fn json_to_expr(value: &serde_json::Value) -> Option<RestrictedExpression> {
    match value {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64()
                    .and_then(|f| RestrictedExpression::new_decimal(f.to_string()).ok())
            }
        }
        serde_json::Value::Array(arr) => {
            let elements: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elements))
        }
        serde_json::Value::Object(map) => {
            let mut record: std::collections::BTreeMap<String, RestrictedExpression> =
                std::collections::BTreeMap::new();
            for (key, val) in map.iter() {
                if let Some(expr) = json_to_expr(val) {
                    record.insert(key.clone(), expr);
                }
            }
            RestrictedExpression::new_record(record).ok()
        }
        serde_json::Value::Null => None,
    }
}
