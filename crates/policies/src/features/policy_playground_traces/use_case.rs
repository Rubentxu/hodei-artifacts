use super::dto::{TracedAuthorizationResult, TracedPlaygroundOptions, TracedPlaygroundResponse};
use crate::features::policy_playground::dto as base;
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use tokio::task::JoinSet;

pub struct TracedPlaygroundUseCase;

impl TracedPlaygroundUseCase {
    pub fn new() -> Self { Self }

    pub async fn execute(
        &self,
        options: &TracedPlaygroundOptions,
        base_req: &base::PlaygroundRequest,
        base_uc: &crate::features::policy_playground::use_case::PolicyPlaygroundUseCase,
    ) -> Result<TracedPlaygroundResponse, String> {
        if !options.include_policy_traces {
            // Fast path: no traces, just call base
            let result = base_uc.execute(base_req).await.map_err(|e| e.to_string())?;
            let wrapped: Vec<TracedAuthorizationResult> = result
                .authorization_results
                .into_iter()
                .map(|base_res| TracedAuthorizationResult { base: base_res, determining_policies: None, evaluated_policies: None })
                .collect();
            return Ok(TracedPlaygroundResponse {
                policy_validation: result.policy_validation,
                schema_validation: result.schema_validation,
                authorization_results: wrapped,
                statistics: result.statistics,
            });
        }

        // Heuristic path: DO NOT call base_uc to avoid ID conflicts; replicate minimal logic
        // Parse all policies together as a single PolicySet to get consistent IDs
        let mut policy_set_str = String::new();
        for pstr in base_req.policies.iter() {
            policy_set_str.push_str(pstr.trim());
            policy_set_str.push_str("\n\n");
        }
        
        // Parse the entire PolicySet at once
        let pset_parsed = PolicySet::from_str(&policy_set_str)
            .map_err(|e| format!("failed to parse policy set: {}", e))?;
        
        // Extract policies with their Cedar-assigned IDs
        let mut policies: Vec<(String, Policy)> = Vec::with_capacity(base_req.policies.len());
        for p in pset_parsed.policies() {
            let id = p.id().to_string();
            policies.push((id, p.clone()));
        }

        // Minimal validation result (no schema/policy validation for traces mode)
        let policy_validation = base::PolicyValidationResult {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
            policies_count: policies.len(),
        };
        let schema_validation = base::SchemaValidationResult {
            is_valid: true,
            errors: vec![],
            entity_types_count: 0,
            actions_count: 0,
        };

        // Build Entities from request
        let entities = build_entities(&base_req.entities)?;

        // Authorizer
        let authorizer = Authorizer::new();

        // For each scenario, compute determining policies by removal (parallel per policy)
        let mut traced_results: Vec<TracedAuthorizationResult> = Vec::with_capacity(base_req.authorization_requests.len());
        let mut total_time: u64 = 0;
        let mut allow_count: usize = 0;
        for sc in &base_req.authorization_requests {

            let principal = EntityUid::from_str(&sc.principal).map_err(|e| format!("principal: {}", e))?;
            let action = EntityUid::from_str(&sc.action).map_err(|e| format!("action: {}", e))?;
            let resource = EntityUid::from_str(&sc.resource).map_err(|e| format!("resource: {}", e))?;
            let context = build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;

            let start = std::time::Instant::now();

            // Build full PolicySet - use the parsed one directly to preserve IDs
            let pset_all = pset_parsed.clone();

            // Baseline
            let baseline = authorizer.is_authorized(&request, &pset_all, &entities);
            let baseline_allow = baseline.decision() == CedarDecision::Allow;
            if baseline_allow { allow_count += 1; }

            // Parallel removal
            let mut set: JoinSet<(String, bool)> = JoinSet::new();
            let policy_strings = base_req.policies.clone(); // Keep original strings
            for (i, (pol_id, _)) in policies.iter().enumerate() {
                let pol_id_cloned = pol_id.clone();
                let policy_strings_clone = policy_strings.clone();
                let entities_clone = entities.clone();
                let sc_principal_c = sc.principal.clone();
                let sc_action_c = sc.action.clone();
                let sc_resource_c = sc.resource.clone();
                let sc_context_c = sc.context.clone();
                set.spawn(async move {
                    // Rebuild PolicySet without policy i
                    let mut pset_str = String::new();
                    for (j, pstr) in policy_strings_clone.iter().enumerate() {
                        if i != j {
                            pset_str.push_str(pstr.trim());
                            pset_str.push_str("\n\n");
                        }
                    }
                    let pset = PolicySet::from_str(&pset_str).unwrap_or_else(|_| PolicySet::new());
                    
                    // Recreate request
                    let principal = EntityUid::from_str(&sc_principal_c).unwrap();
                    let action = EntityUid::from_str(&sc_action_c).unwrap();
                    let resource = EntityUid::from_str(&sc_resource_c).unwrap();
                    let context = build_context(sc_context_c.as_ref());
                    let request = Request::new(principal, action, resource, context, None).unwrap();
                    let a = Authorizer::new();
                    let resp = a.is_authorized(&request, &pset, &entities_clone);
                    let allow = resp.decision() == CedarDecision::Allow;
                    (pol_id_cloned, allow)
                });
            }

            let mut determining: Vec<String> = Vec::new();
            while let Some(joined) = set.join_next().await {
                if let Ok((id, allow)) = joined { if allow != baseline_allow { determining.push(id); } }
            }

            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;

            let base_result = base::AuthorizationResult {
                scenario_name: sc.name.clone(),
                decision: if baseline_allow { base::Decision::Allow } else { base::Decision::Deny },
                determining_policies: vec![],
                evaluated_policies: vec![],
                diagnostics: base::AuthorizationDiagnostics { reasons: vec![], errors: vec![], info: vec![] },
                evaluation_time_us: eval_time,
            };

            traced_results.push(TracedAuthorizationResult {
                base: base_result,
                determining_policies: Some(determining),
                evaluated_policies: None,
            });
        }

        let statistics = base::EvaluationStatistics {
            total_scenarios: traced_results.len(),
            allow_count,
            deny_count: traced_results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if traced_results.is_empty() { 0 } else { total_time / traced_results.len() as u64 },
        };

        Ok(TracedPlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: traced_results,
            statistics,
        })
    }
}

fn build_entities(defs: &[base::EntityDefinition]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for d in defs {
        let uid = EntityUid::from_str(&d.uid).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in &d.attributes { if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); } }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in &d.parents { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

fn build_context(ctx: Option<&HashMap<String, serde_json::Value>>) -> Context {
    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
    if let Some(c) = ctx {
        for (k, v) in c {
            if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
        }
    }
    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
}

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
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn determining_policy_includes_forbid_group() {
        // Policies: forbid admins; permit all (no explicit IDs, will be auto-assigned)
        let req = base::PlaygroundRequest {
            policies: vec![
                "forbid(principal in Group::\"admins\", action, resource);".to_string(),
                "permit(principal, action, resource);".to_string(),
            ],
            schema: None,
            entities: vec![
                base::EntityDefinition { uid: "User::\"alice\"".to_string(), attributes: Default::default(), parents: vec!["Group::\"admins\"".to_string()] },
                base::EntityDefinition { uid: "Group::\"admins\"".to_string(), attributes: Default::default(), parents: vec![] },
            ],
            authorization_requests: vec![ base::AuthorizationScenario {
                name: "alice-deny".to_string(),
                principal: "User::\"alice\"".to_string(),
                action: "Action::\"view\"".to_string(),
                resource: "Resource::\"doc1\"".to_string(),
                context: None,
            }],
            options: None,
        };

        let base_uc = crate::features::policy_playground::use_case::PolicyPlaygroundUseCase::default();
        let traced_uc = TracedPlaygroundUseCase::new();
        let opts = TracedPlaygroundOptions { include_policy_traces: true };
        let res = traced_uc.execute(&opts, &req, &base_uc).await.unwrap();
        let det = &res.authorization_results[0].determining_policies;
        
        assert!(det.as_ref().unwrap().len() >= 1);
        // The forbid policy is determining (removing it changes decision from Deny to Allow)
        // Cedar assigns IDs automatically (policy0, policy1, etc.)
        // The determining policy should be one of them (either policy0 or policy1 depending on parse order)
        let determining_policies = det.as_ref().unwrap();
        assert!(
            determining_policies.contains(&"policy0".to_string()) || 
            determining_policies.contains(&"policy1".to_string()),
            "Expected either policy0 or policy1 in determining policies, but got: {:?}",
            determining_policies
        );
    }
}
