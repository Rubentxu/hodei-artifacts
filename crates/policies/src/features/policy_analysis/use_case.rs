use super::dto::{AnalyzePoliciesRequest, AnalyzePoliciesResponse, RuleViolation};
use crate::shared::application::parallel::{evaluate_until_first, AuthScenario};
use cedar_policy::{
    Entities, EntityUid, Policy, PolicySet, Schema, SchemaFragment, ValidationMode, Validator,
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
        if let Some(s) = &req.schema {
            if let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s) {
                if let Ok(schema) = Schema::from_schema_fragments(vec![frag]) {
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
                    let scenarios = vec![
                        AuthScenario {
                            name: "mfa_false".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: Some(ctx_false),
                        },
                        AuthScenario {
                            name: "mfa_missing".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        },
                    ];
                    if let Some(out) = evaluate_until_first(
                        &pset,
                        &Entities::empty(),
                        scenarios,
                        None,
                        4,
                        8,
                        |o| o.allow,
                    )
                    .await?
                    {
                        violations.push(RuleViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Allow without strong auth: scenario='{}' P='{}' A='{}' R='{}'",
                                out.name, principal, action, resource
                            ),
                        });
                    }
                }
                "no_permit_without_condition" => {
                    let unconditioned = req.policies.iter().enumerate().any(|(_i, p)| {
                        let pol = p.to_lowercase();
                        pol.contains("permit(")
                            && !pol.contains(" when ")
                            && !pol.contains("unless ")
                    });
                    if unconditioned {
                        let principal = synth_euid("User", "u").to_string();
                        let action = synth_euid("Action", "a").to_string();
                        let resource = synth_euid("Resource", "r").to_string();
                        let scenarios = vec![AuthScenario {
                            name: "empty_ctx".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        }];
                        if let Some(out) = evaluate_until_first(
                            &pset,
                            &Entities::empty(),
                            scenarios,
                            None,
                            2,
                            4,
                            |o| o.allow,
                        )
                        .await?
                        {
                            violations.push(RuleViolation {
                                rule_id: rule.id.clone(),
                                message: format!(
                                    "Allow without condition: scenario='{}' P='{}' A='{}' R='{}'",
                                    out.name, principal, action, resource
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn analysis_detects_permit_without_mfa() {
        let req = AnalyzePoliciesRequest {
            policies: vec!["permit(principal, action, resource);".to_string()],
            schema: None,
            rules: vec![AnalysisRule {
                id: "r1".to_string(),
                kind: "no_permit_without_mfa".to_string(),
                params: serde_json::json!({}),
            }],
        };
        let uc = AnalyzePoliciesUseCase::new();
        let res = uc.execute(&req).await.unwrap();
        assert!(!res.passed);
        assert!(!res.violations.is_empty());
    }

    #[tokio::test]
    async fn analysis_passes_with_mfa_when() {
        let req = AnalyzePoliciesRequest {
            policies: vec![
                "permit(principal, action, resource) when { context.mfa == true };".to_string(),
            ],
            schema: None,
            rules: vec![AnalysisRule {
                id: "r1".to_string(),
                kind: "no_permit_without_mfa".to_string(),
                params: serde_json::json!({}),
            }],
        };
        let uc = AnalyzePoliciesUseCase::new();
        let res = uc.execute(&req).await.unwrap();
        assert!(res.passed);
    }
}
