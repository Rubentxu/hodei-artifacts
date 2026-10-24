//! Simplified Authorization Engine Core Implementation
//!
//! This module implements a basic Cedar-based authorization engine that works
//! with the current Cedar API and compiles successfully.
//!
//! # Statelessness
//!
//! The engine keeps **no mutable state between requests**. Policies and
//! entities are compiled into an immutable [`EvaluationSnapshot`] that is
//! built **per request** and handed to [`AuthorizationEngine::evaluate`].
//! Two concurrent evaluations can never observe each other's policies or
//! entities: there is simply no shared registry to read from or write to.
//!
//! ## Schema-less Operation
//!
//! By default, this engine operates without a Cedar schema, which means:
//! - Policies are evaluated based on runtime entity data
//! - Actions are referenced with the flat `Action::"Name"` form
//! - No compile-time validation of principal/resource types
//! - Maximum flexibility - any entity type can use any action
//!
//! ## When to Use Schema
//!
//! If you need Cedar's schema validation (type checking, action constraints),
//! you should:
//! 1. Register actions using types that implement `kernel::ActionTrait`
//! 2. Build a schema using `EngineBuilder`
//! 3. Pass the schema explicitly during evaluation
//!
//! Applying the schema to the request is scheduled for H1b (see
//! `ADR/011-h1a-identity-and-decision-core.md`).

use super::translator;
use super::types::{AuthorizationDecision, EngineError, EngineRequest, PolicyDocument};
use cedar_policy::{Authorizer, Context, Entities, Policy, PolicySet, Request};
use kernel::HodeiEntity;
use tracing::{debug, info};

/// Simple Authorization Engine
///
/// The engine is a thin wrapper around Cedar's [`Authorizer`]: it holds no
/// policies and no entities. Everything it needs comes from the
/// [`EvaluationSnapshot`] the caller passes to [`AuthorizationEngine::evaluate`].
pub struct AuthorizationEngine {
    /// Cedar authorizer
    authorizer: Authorizer,
}

/// Immutable snapshot of the policies and entities of one evaluation
///
/// The snapshot is built once per request and never mutated afterwards, so
/// it cannot be contaminated by another request.
pub struct EvaluationSnapshot {
    policies: PolicySet,
    entities: Entities,
}

impl EvaluationSnapshot {
    /// Number of policies in the snapshot
    pub fn policy_count(&self) -> usize {
        self.policies.policies().count()
    }

    /// Number of entities in the snapshot
    pub fn entity_count(&self) -> usize {
        self.entities.iter().count()
    }
}

impl AuthorizationEngine {
    /// Create a new authorization engine
    pub fn new() -> Self {
        Self {
            authorizer: Authorizer::new(),
        }
    }

    /// Build the immutable snapshot for a single evaluation
    ///
    /// The snapshot compiles the policies and translates the entities once,
    /// so the evaluation itself does not pay for it and cannot observe a
    /// different snapshot than the one it was given.
    ///
    /// # Errors
    ///
    /// Returns an error if a policy cannot be parsed or added to the policy
    /// set, or if an entity cannot be translated to Cedar.
    pub fn build_snapshot(
        &self,
        policies: &[PolicyDocument],
        entities: &[&dyn HodeiEntity],
    ) -> Result<EvaluationSnapshot, EngineError> {
        let mut policy_set = PolicySet::new();

        for policy in policies {
            let cedar_policy = Policy::parse(
                Some(cedar_policy::PolicyId::new(policy.id())),
                policy.content(),
            )
            .map_err(|e| {
                EngineError::InvalidPolicy(format!("Policy {} parse error: {}", policy.id(), e))
            })?;

            policy_set.add(cedar_policy).map_err(|e| {
                EngineError::InvalidPolicy(format!("Failed to add policy {}: {}", policy.id(), e))
            })?;
        }

        let cedar_entities: Result<Vec<_>, _> = entities
            .iter()
            .map(|entity| translator::translate_to_cedar_entity(*entity))
            .collect();

        let cedar_entities =
            cedar_entities.map_err(|e| EngineError::TranslationError(e.to_string()))?;

        let entity_store = Entities::from_entities(cedar_entities, None).map_err(|e| {
            EngineError::TranslationError(format!("Failed to create entities: {}", e))
        })?;

        debug!(
            policies = policy_set.policies().count(),
            entities = entity_store.iter().count(),
            "Built evaluation snapshot"
        );

        Ok(EvaluationSnapshot {
            policies: policy_set,
            entities: entity_store,
        })
    }

    /// Evaluate an authorization request against an immutable snapshot
    ///
    /// This method evaluates policies without Cedar schema validation.
    /// Actions are treated as strings and entities are validated only
    /// against the policy content itself.
    ///
    /// This approach provides maximum flexibility while maintaining
    /// Cedar's powerful policy evaluation capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if the entities or the action cannot be translated,
    /// or if the Cedar request cannot be built.
    pub fn evaluate(
        &self,
        snapshot: &EvaluationSnapshot,
        request: &EngineRequest<'_>,
    ) -> Result<AuthorizationDecision, EngineError> {
        debug!("Starting authorization evaluation");

        // 1. Translate entities to Cedar
        let principal_cedar = translator::translate_to_cedar_entity(request.principal)
            .map_err(|e| EngineError::TranslationError(e.to_string()))?;
        let resource_cedar = translator::translate_to_cedar_entity(request.resource)
            .map_err(|e| EngineError::TranslationError(e.to_string()))?;

        debug!("Translated entities successfully");

        // 2. Build Cedar action EntityUid
        // The action namespace is flat, the same form the registered schema
        // declares and the same form `ActionTrait::action_name()` produces.
        let action_uid = translator::translate_action_to_cedar_euid(request.action)
            .map_err(|e| EngineError::EvaluationFailed(format!("Invalid action: {}", e)))?;

        // 3. Build Cedar Context from request context
        let cedar_context = if request.context.is_empty() {
            Context::empty()
        } else {
            // Convert HashMap<String, serde_json::Value> to Cedar Context
            let mut context_map = std::collections::HashMap::new();
            for (key, value) in &request.context {
                // Convert serde_json::Value to RestrictedExpression
                let restricted_expr = json_value_to_restricted_expr(value).map_err(|e| {
                    EngineError::EvaluationFailed(format!("Context conversion error: {}", e))
                })?;
                context_map.insert(key.clone(), restricted_expr);
            }
            cedar_policy::Context::from_pairs(context_map).map_err(|e| {
                EngineError::EvaluationFailed(format!("Failed to build context: {}", e))
            })?
        };

        // 4. Build Cedar Request in schema-less mode
        // We operate without schema validation, which allows:
        // - Any principal type to use any action
        // - Actions defined as strings without ActionTrait types
        // - Maximum flexibility in policy evaluation
        // Cedar evaluates policies based on entity attributes and policy conditions
        let cedar_request = Request::new(
            principal_cedar.uid().clone(),
            action_uid,
            resource_cedar.uid().clone(),
            cedar_context,
            None, // Schema-less mode: no type validation
        )
        .map_err(|e| EngineError::EvaluationFailed(format!("Failed to build request: {}", e)))?;

        // 5. Evaluate with Cedar against the request's own snapshot
        let response =
            self.authorizer
                .is_authorized(&cedar_request, &snapshot.policies, &snapshot.entities);
        debug!("Cedar evaluation complete: {:?}", response.decision());

        // 6. Map response to decision
        let determining_policies: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|policy_id| policy_id.to_string())
            .collect();

        let errors: Vec<String> = response
            .diagnostics()
            .errors()
            .map(|error| error.to_string())
            .collect();

        let decision = match response.decision() {
            cedar_policy::Decision::Allow => {
                info!("Authorization ALLOWED");
                if errors.is_empty() {
                    AuthorizationDecision::allow()
                } else {
                    AuthorizationDecision::allow_with_reason(errors.join("; "))
                }
            }
            cedar_policy::Decision::Deny => {
                info!("Authorization DENIED");
                if errors.is_empty() {
                    AuthorizationDecision::deny()
                } else {
                    AuthorizationDecision::deny_with_reason(errors.join("; "))
                }
            }
        };

        Ok(decision.with_policies(determining_policies))
    }
}

impl Default for AuthorizationEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to convert serde_json::Value to Cedar RestrictedExpression
fn json_value_to_restricted_expr(
    value: &serde_json::Value,
) -> Result<cedar_policy::RestrictedExpression, String> {
    use serde_json::Value;

    match value {
        Value::Null => Err("Null values not supported in Cedar context".to_string()),
        Value::Bool(b) => Ok(cedar_policy::RestrictedExpression::new_bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(cedar_policy::RestrictedExpression::new_long(i))
            } else {
                Err(format!("Number {} cannot be converted to i64", n))
            }
        }
        Value::String(s) => Ok(cedar_policy::RestrictedExpression::new_string(s.clone())),
        Value::Array(arr) => {
            let exprs: Result<Vec<_>, _> = arr.iter().map(json_value_to_restricted_expr).collect();
            Ok(cedar_policy::RestrictedExpression::new_set(exprs?))
        }
        Value::Object(map) => {
            let mut cedar_map = std::collections::HashMap::new();
            for (key, val) in map {
                let expr = json_value_to_restricted_expr(val)?;
                cedar_map.insert(key.clone(), expr);
            }
            cedar_policy::RestrictedExpression::new_record(cedar_map)
                .map_err(|e| format!("Failed to create record: {}", e))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{
        AttributeName, AttributeType, AttributeValue, ResourceTypeName, ServiceName,
    };
    use kernel::{HodeiEntity, HodeiEntityType, Hrn};
    use std::collections::HashMap;

    // Test entity
    #[derive(Debug)]
    struct TestUser {
        hrn: Hrn,
        name: String,
    }

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![(AttributeName::new("name").unwrap(), AttributeType::string())]
        }
    }

    impl HodeiEntity for TestUser {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("name").unwrap(),
                AttributeValue::string(&self.name),
            );
            attrs
        }
    }

    fn user(account: &str, name: &str) -> TestUser {
        TestUser {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                account.to_string(),
                "User".to_string(),
                name.to_string(),
            ),
            name: name.to_string(),
        }
    }

    fn policy(id: &str, content: &str) -> PolicyDocument {
        PolicyDocument::new(id.to_string(), content.to_string())
    }

    #[test]
    fn engine_creation() {
        let engine = AuthorizationEngine::new();
        let snapshot = engine
            .build_snapshot(&[], &[])
            .expect("an empty snapshot must build");
        assert_eq!(snapshot.policy_count(), 0);
        assert_eq!(snapshot.entity_count(), 0);
    }

    #[test]
    fn a_snapshot_carries_the_policies_and_entities_it_was_built_with() {
        let engine = AuthorizationEngine::new();
        let alice = user("123", "alice");

        let snapshot = engine
            .build_snapshot(
                &[policy("p1", "permit(principal, action, resource);")],
                &[&alice],
            )
            .expect("snapshot");

        assert_eq!(snapshot.policy_count(), 1);
        assert_eq!(snapshot.entity_count(), 1);
    }

    #[test]
    fn an_invalid_policy_fails_the_snapshot() {
        let engine = AuthorizationEngine::new();

        let result = engine.build_snapshot(&[policy("bad", "this is not cedar")], &[]);

        assert!(result.is_err());
    }

    #[test]
    fn a_permit_allows() {
        let engine = AuthorizationEngine::new();
        let alice = user("123", "alice");
        let snapshot = engine
            .build_snapshot(
                &[policy("p1", "permit(principal, action, resource);")],
                &[&alice],
            )
            .expect("snapshot");

        let request = EngineRequest::new(&alice, "Read", &alice);
        let decision = engine.evaluate(&snapshot, &request).expect("decision");

        assert!(decision.is_allowed());
        assert_eq!(decision.determining_policies(), ["p1"]);
    }

    #[test]
    fn an_empty_policy_set_denies() {
        let engine = AuthorizationEngine::new();
        let alice = user("123", "alice");
        let snapshot = engine.build_snapshot(&[], &[&alice]).expect("snapshot");

        let request = EngineRequest::new(&alice, "Read", &alice);
        let decision = engine.evaluate(&snapshot, &request).expect("decision");

        assert!(!decision.is_allowed());
    }

    /// El `forbid` explícito prevalece sobre el `permit`.
    #[test]
    fn an_explicit_forbid_wins_over_a_permit() {
        let engine = AuthorizationEngine::new();
        let alice = user("123", "alice");
        let snapshot = engine
            .build_snapshot(
                &[
                    policy("permit_all", "permit(principal, action, resource);"),
                    policy(
                        "forbid_alice",
                        "forbid(principal, action, resource) when { principal.name == \"alice\" };",
                    ),
                ],
                &[&alice],
            )
            .expect("snapshot");

        let request = EngineRequest::new(&alice, "Read", &alice);
        let decision = engine.evaluate(&snapshot, &request).expect("decision");

        assert!(!decision.is_allowed());
        assert_eq!(decision.determining_policies(), ["forbid_alice"]);
    }

    /// Dos recursos homónimos en cuentas distintas son entidades distintas:
    /// una política que nombra a una cuenta no alcanza a la otra.
    #[test]
    fn a_policy_scoped_to_one_account_does_not_reach_another_account() {
        let engine = AuthorizationEngine::new();
        let alice_in_a = user("account-a", "alice");
        let alice_in_b = user("account-b", "alice");

        let snapshot = engine
            .build_snapshot(
                &[policy(
                    "only_account_a",
                    "permit(principal, action, resource) when { principal == Iam::User::\"hrn:aws:iam::account-a:User/alice\" };",
                )],
                &[&alice_in_a, &alice_in_b],
            )
            .expect("snapshot");

        let request_a = EngineRequest::new(&alice_in_a, "Read", &alice_in_a);
        let decision_a = engine.evaluate(&snapshot, &request_a).expect("decision a");
        assert!(decision_a.is_allowed());

        let request_b = EngineRequest::new(&alice_in_b, "Read", &alice_in_b);
        let decision_b = engine.evaluate(&snapshot, &request_b).expect("decision b");
        assert!(
            !decision_b.is_allowed(),
            "a policy scoped to account-a must not grant account-b"
        );
    }

    /// Dos evaluaciones concurrentes con políticas opuestas no se contaminan.
    #[test]
    fn concurrent_evaluations_do_not_contaminate_each_other() {
        use std::sync::Arc;

        let engine = Arc::new(AuthorizationEngine::new());
        let alice = Arc::new(user("123", "alice"));

        let allow_snapshot = engine
            .build_snapshot(
                &[policy("permit_all", "permit(principal, action, resource);")],
                &[alice.as_ref() as &dyn HodeiEntity],
            )
            .expect("allow snapshot");

        let deny_snapshot = engine
            .build_snapshot(
                &[policy("forbid_all", "forbid(principal, action, resource);")],
                &[alice.as_ref() as &dyn HodeiEntity],
            )
            .expect("deny snapshot");

        let handles: Vec<_> = (0..8)
            .map(|index| {
                let engine = Arc::clone(&engine);
                let alice = Arc::clone(&alice);
                let allow_snapshot = EvaluationSnapshot {
                    policies: allow_snapshot.policies.clone(),
                    entities: allow_snapshot.entities.clone(),
                };
                let deny_snapshot = EvaluationSnapshot {
                    policies: deny_snapshot.policies.clone(),
                    entities: deny_snapshot.entities.clone(),
                };
                std::thread::spawn(move || {
                    let request = EngineRequest::new(alice.as_ref(), "Read", alice.as_ref());
                    let decision = if index % 2 == 0 {
                        engine
                            .evaluate(&allow_snapshot, &request)
                            .expect("decision")
                    } else {
                        engine.evaluate(&deny_snapshot, &request).expect("decision")
                    };
                    decision.is_allowed()
                })
            })
            .collect();

        for (index, handle) in handles.into_iter().enumerate() {
            let allowed = handle.join().expect("thread");
            assert_eq!(allowed, index % 2 == 0, "decision {index} was contaminated");
        }
    }
}
