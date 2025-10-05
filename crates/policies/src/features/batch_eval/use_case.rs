use super::dto::{BatchPlaygroundRequest, BatchPlaygroundResponse};
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use crate::features::policy_playground::dto as base;
use tracing::info;

#[derive(Default)]
pub struct BatchEvalUseCase;

impl BatchEvalUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &BatchPlaygroundRequest,
    ) -> Result<BatchPlaygroundResponse, String> {
        // Apply limit
        let scenarios = if let Some(limit) = req.limit_scenarios {
            req.scenarios
                .iter()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            req.scenarios.clone()
        };

        // Build PolicySet and Entities
        let pset = self.build_policy_set(&req.policies)?;
        let ents = self.build_entities(&req.entities)?;

        // Evaluate scenarios sequentially
        let total = scenarios.len();
        let authorizer = Authorizer::new();
        let mut allow_count = 0usize;
        let mut total_time = 0u64;
        let mut timeouts = 0usize;

        for scenario in scenarios {
            let start = Instant::now();
            
            // Build request components
            let principal = EntityUid::from_str(&scenario.principal)
                .map_err(|e| format!("principal parse error: {}", e))?;
            let action = EntityUid::from_str(&scenario.action)
                .map_err(|e| format!("action parse error: {}", e))?;
            let resource = EntityUid::from_str(&scenario.resource)
                .map_err(|e| format!("resource parse error: {}", e))?;
            
            // Build context
            let context = self.build_context(scenario.context.as_ref());
            
            let request = Request::new(principal, action, resource, context, None)
                .map_err(|e| format!("request build error: {}", e))?;
            
            // Evaluate authorization
            let resp = authorizer.is_authorized(&request, &pset, &ents);
            let allow = resp.decision() == CedarDecision::Allow;
            
            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;
            
            if allow {
                allow_count += 1;
            }
        }

        let statistics = base::EvaluationStatistics {
            total_scenarios: total,
            allow_count,
            deny_count: total.saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if total == 0 {
                0
            } else {
                total_time / total as u64
            },
        };

        info!(
            scenarios_total = total,
            timeouts = timeouts,
            total_eval_time_us = total_time,
            "batch_eval completed"
        );

        Ok(BatchPlaygroundResponse {
            results_count: total,
            statistics,
        })
    }

    fn build_policy_set(&self, policies: &[String]) -> Result<PolicySet, String> {
        let mut pset = PolicySet::new();
        for (i, pstr) in policies.iter().enumerate() {
            let pol: Policy = pstr
                .parse()
                .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
            pset.add(pol)
                .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
        }
        Ok(pset)
    }

    fn build_entities(&self, defs: &[base::EntityDefinition]) -> Result<Entities, String> {
        if defs.is_empty() { return Ok(Entities::empty()); }
        let mut out = Vec::with_capacity(defs.len());
        for d in defs {
            let uid = EntityUid::from_str(&d.uid).map_err(|e| format!("entity uid parse error: {}", e))?;
            let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
            for (k, v) in &d.attributes {
                if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
            }
            let mut parents: HashSet<EntityUid> = HashSet::new();
            for p in &d.parents {
                parents.insert(EntityUid::from_str(p).map_err(|e| format!("parent uid parse error: {}", e))?);
            }
            let ent = Entity::new(uid, attrs, parents).map_err(|e| format!("entity creation error: {}", e))?;
            out.push(ent);
        }
        Entities::from_entities(out, None).map_err(|e| format!("entities creation error: {}", e))
    }

    fn build_context(&self, ctx: Option<&HashMap<String, serde_json::Value>>) -> Context {
        let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
        if let Some(c) = ctx {
            for (k, v) in c {
                if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
            }
        }
        Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
    }
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
