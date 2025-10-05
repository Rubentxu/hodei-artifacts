use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression, Schema, SchemaFragment, ValidationMode, Validator};

use super::dto::{
    AuthorizationDiagnostics, AuthorizationResult, Decision, EntityDefinition, EvaluationStatistics,
    PlaygroundRequest, PlaygroundResponse, PolicyValidationResult, SchemaValidationResult,
    ValidationError, ValidationWarning,
};

#[derive(Debug, thiserror::Error)]
pub enum PlaygroundError {
    #[error("invalid_request: {0}")]
    InvalidRequest(String),
    #[error("policy_parse_error: {0}")]
    PolicyParseError(String),
    #[error("euid_parse_error: {0}")]
    EuidParseError(String),
    #[error("request_build_error: {0}")]
    RequestError(String),
    #[error("schema_parse_error: {0}")]
    SchemaParseError(String),
    #[error("entity_parse_error: {0}")]
    EntityParseError(String),
}

// Helper: map serde_json::Value to RestrictedExpression (basic types)
fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: std::collections::BTreeMap<String, RestrictedExpression> = std::collections::BTreeMap::new();
            for (k, val) in map.iter() {
                if let Some(expr) = json_to_expr(val) {
                    rec.insert(k.clone(), expr);
                }
            }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

impl Default for PolicyPlaygroundUseCase {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PolicyPlaygroundUseCase;

impl PolicyPlaygroundUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &PlaygroundRequest,
    ) -> Result<PlaygroundResponse, PlaygroundError> {
        if req.policies.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one policy is required".to_string(),
            ));
        }
        if req.authorization_requests.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one authorization scenario is required".to_string(),
            ));
        }

        // Schema handling
        let schema_validation = self.validate_schema(&req.schema)?;

        // Parse and validate policies
        let (pset, policy_validation) = self.parse_and_validate_policies(&req.policies, &req.schema)?;

        // Build entities
        let entities = self.parse_entities(&req.entities)?;

        // Evaluate scenarios sequentially
        let mut results = Vec::with_capacity(req.authorization_requests.len());
        let mut total_time = 0u64;
        let mut allow_count = 0usize;

        let authorizer = Authorizer::new();
        for sc in &req.authorization_requests {
            let start = Instant::now();
            let principal = EntityUid::from_str(&sc.principal)
                .map_err(|e| PlaygroundError::EuidParseError(format!("principal: {}", e)))?;
            let action = EntityUid::from_str(&sc.action)
                .map_err(|e| PlaygroundError::EuidParseError(format!("action: {}", e)))?;
            let resource = EntityUid::from_str(&sc.resource)
                .map_err(|e| PlaygroundError::EuidParseError(format!("resource: {}", e)))?;
            let context = self.build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None)
                .map_err(|e| PlaygroundError::RequestError(e.to_string()))?;
            let resp = authorizer.is_authorized(&request, &pset, &entities);
            let decision = if resp.decision() == CedarDecision::Allow { allow_count += 1; Decision::Allow } else { Decision::Deny };
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;
            results.push(AuthorizationResult { scenario_name: sc.name.clone(), decision, determining_policies: vec![], evaluated_policies: vec![], diagnostics: AuthorizationDiagnostics { reasons, errors: vec![], info: vec![] }, evaluation_time_us: eval_time });
        }

        // Stable order for determinism in tests
        results.sort_by(|a, b| a.scenario_name.cmp(&b.scenario_name));

        let statistics = EvaluationStatistics {
            total_scenarios: results.len(),
            allow_count,
            deny_count: results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if results.is_empty() { 0 } else { total_time / results.len() as u64 },
        };

        Ok(PlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: results,
            statistics,
        })
    }

    fn validate_schema(&self, schema_str: &Option<String>) -> Result<SchemaValidationResult, PlaygroundError> {
        if let Some(s) = schema_str {
            let (frag, _warnings) = SchemaFragment::from_cedarschema_str(s)
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            let _schema = Schema::from_schema_fragments(vec![frag])
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        } else {
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        }
    }

    fn parse_and_validate_policies(
        &self,
        policies: &[String],
        schema: &Option<String>,
    ) -> Result<(PolicySet, PolicyValidationResult), PlaygroundError> {
        let mut pset = PolicySet::new();
        let mut errors = Vec::new();
        let warnings = Vec::<ValidationWarning>::new();

        for (idx, pstr) in policies.iter().enumerate() {
            match pstr.parse::<Policy>() {
                Ok(pol) => {
                    if let Err(e) = pset.add(pol) {
                        errors.push(ValidationError { message: format!("add error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None });
                    }
                }
                Err(e) => errors.push(ValidationError { message: format!("parse error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None }),
            }
        }

        if errors.is_empty()
            && let Some(s) = schema
            && let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s)
            && let Ok(schema_obj) = Schema::from_schema_fragments(vec![frag])
        {
            let validator = Validator::new(schema_obj);
            let vr = validator.validate(&pset, ValidationMode::default());
            if !vr.validation_passed() {
                for e in vr.validation_errors() {
                    errors.push(ValidationError { message: e.to_string(), policy_id: None, line: None, column: None });
                }
            }
        }

        Ok((
            pset,
            PolicyValidationResult { is_valid: errors.is_empty(), errors, warnings, policies_count: policies.len() },
        ))
    }

    fn parse_entities(&self, defs: &[EntityDefinition]) -> Result<Entities, PlaygroundError> {
        if defs.is_empty() { return Ok(Entities::empty()); }
        let mut out = Vec::with_capacity(defs.len());
        for d in defs {
            let uid = EntityUid::from_str(&d.uid)
                .map_err(|e| PlaygroundError::EntityParseError(format!("{}", e)))?;
            let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
            for (k, v) in &d.attributes {
                if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
            }
            let mut parents: HashSet<EntityUid> = HashSet::new();
            for p in &d.parents {
                parents.insert(EntityUid::from_str(p).map_err(|e| PlaygroundError::EntityParseError(format!("parent: {}", e)))?);
            }
            let ent = Entity::new(uid, attrs, parents).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))?;
            out.push(ent);
        }
        Entities::from_entities(out, None).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))
    }

    fn build_context(&self, ctx: Option<&std::collections::HashMap<String, serde_json::Value>>) -> Context {
        let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
        if let Some(c) = ctx {
            for (k, v) in c {
                if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
            }
        }
        Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
    }
}
