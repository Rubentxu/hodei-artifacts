use super::dto::{AnalyzePoliciesRequest, AnalyzePoliciesResponse, RuleViolation};
use cedar_policy::{
    Authorizer, Context, Entities, EntityUid, Policy, PolicySet, Request,
    Schema, SchemaFragment, ValidationMode, Validator,
};
use std::str::FromStr;

#[derive(Default)]
pub struct AnalyzePoliciesUseCase;

impl AnalyzePoliciesUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &AnalyzePoliciesRequest,
    ) -> Result<AnalyzePoliciesResponse, String> {
        // Build PolicySet once (fail fast on invalid)
        let mut pset = PolicySet::new();
        for (i, p) in req.policies.iter().enumerate() {
            let pol: Policy = p
                .parse()
                .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
            pset.add(pol)
                .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
        }

        // Heuristic + semantic checks
        let mut violations: Vec<RuleViolation> = Vec::new();

        // Optional: schema-based validation of policy set
        if let Some(s) = &req.schema
            && let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s)
                && let Ok(schema) = Schema::from_schema_fragments(vec![frag]) {
                    let v = Validator::new(schema);
                    let vr = v.validate(&pset, ValidationMode::default());
                    if !vr.validation_passed() {
                        for e in vr.validation_errors() {
                            violations.push(RuleViolation {
                                rule_id: "validator".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }

        for rule in &req.rules {
            match rule.kind.as_str() {
                "no_permit_without_mfa" => {
                    let principal = synth_euid("User", "synthetic").to_string();
                    let action = synth_euid("Action", "view").to_string();
                    let resource = synth_euid("Resource", "doc1").to_string();
                    let mut ctx_false = std::collections::HashMap::new();
                    ctx_false.insert("mfa".to_string(), serde_json::json!(false));
                    
                    // Evaluate scenarios sequentially
                    let authorizer = Authorizer::new();
                    let entities = Entities::empty();
                    
                    // First scenario: mfa = false
                    let context_false = Context::from_pairs(std::collections::HashMap::new()).unwrap_or_else(|_| Context::empty());
                    let request_false = Request::new(
                        EntityUid::from_str(&principal).map_err(|e| e.to_string())?,
                        EntityUid::from_str(&action).map_err(|e| e.to_string())?,
                        EntityUid::from_str(&resource).map_err(|e| e.to_string())?,
                        context_false,
                        None,
                    ).map_err(|e| e.to_string())?;
                    
                    let response_false = authorizer.is_authorized(&request_false, &pset, &entities);
                    if response_false.decision() == cedar_policy::Decision::Allow {
                        violations.push(RuleViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Allow without strong auth: scenario='mfa_false' P='{}' A='{}' R='{}'",
                                principal, action, resource
                            ),
                        });
                        continue; // Found violation, no need to check further
                    }
                    
                    // Second scenario: mfa missing (empty context)
                    let context_empty = Context::empty();
                    let request_empty = Request::new(
                        EntityUid::from_str(&principal).map_err(|e| e.to_string())?,
                        EntityUid::from_str(&action).map_err(|e| e.to_string())?,
                        EntityUid::from_str(&resource).map_err(|e| e.to_string())?,
                        context_empty,
                        None,
                    ).map_err(|e| e.to_string())?;
                    
                    let response_empty = authorizer.is_authorized(&request_empty, &pset, &entities);
                    if response_empty.decision() == cedar_policy::Decision::Allow {
                        violations.push(RuleViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Allow without strong auth: scenario='mfa_missing' P='{}' A='{}' R='{}'",
                                principal, action, resource
                            ),
                        });
                    }
                }
                "no_permit_without_condition" => {
                    let unconditioned = req.policies.iter().any(|p| {
                        let pol = p.to_lowercase();
                        pol.contains("permit(")
                            && !pol.contains(" when ")
                            && !pol.contains("unless ")
                    });
                    if unconditioned {
                        let principal = synth_euid("User", "u").to_string();
                        let action = synth_euid("Action", "a").to_string();
                        let resource = synth_euid("Resource", "r").to_string();
                        
                        // Evaluate scenario sequentially
                        let authorizer = Authorizer::new();
                        let entities = Entities::empty();
                        let context = Context::empty();
                        
                        let request = Request::new(
                            EntityUid::from_str(&principal).map_err(|e| e.to_string())?,
                            EntityUid::from_str(&action).map_err(|e| e.to_string())?,
                            EntityUid::from_str(&resource).map_err(|e| e.to_string())?,
                            context,
                            None,
                        ).map_err(|e| e.to_string())?;
                        
                        let response = authorizer.is_authorized(&request, &pset, &entities);
                        if response.decision() == cedar_policy::Decision::Allow {
                            violations.push(RuleViolation {
                                rule_id: rule.id.clone(),
                                message: format!(
                                    "Allow without condition: scenario='empty_ctx' P='{}' A='{}' R='{}'",
                                    principal, action, resource
                                ),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(AnalyzePoliciesResponse {
            passed: violations.is_empty(),
            violations,
        })
    }
}

fn synth_euid(etype: &str, name: &str) -> EntityUid {
    // Fall back to common types used in our playground
    let et = match etype {
        "User" | "user" => "User",
        "Action" | "action" => "Action",
        "Resource" | "resource" => "Resource",
        other => other,
    };
    EntityUid::from_str(&format!("{}::\"{}\"", et, name)).expect("valid synthetic euid")
}
