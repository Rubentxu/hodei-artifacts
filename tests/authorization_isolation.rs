//! Port-level isolation contract for the authorization evaluation.
//!
//! # Why this file exists
//!
//! The evaluation use case used to keep the policies and the entities of the
//! last request in the engine, behind a shared lock, and to replace them on
//! every call. Two concurrent requests could therefore observe each other's
//! policies. The engine is now stateless and every evaluation runs against an
//! immutable snapshot built for that request, but a regression that put the
//! shared state back would only be caught if the guarantee is asserted at the
//! level a consumer actually uses: the port.
//!
//! # Guarantees
//!
//! - A1 Two concurrent evaluations over the **same** port instance, with
//!   opposite policies, each get the decision of their own request: neither
//!   the decision nor the determining policy leaks between them.
//! - A2 A policy scoped to one account does not reach a homonym of another
//!   account: the tenant boundary is structural, not a runtime check.
//! - A3 An evaluation with no policy is denied (default deny).
//!
//! The test binds no port and spawns no process: the port is the real use
//! case, driven in process.

use std::collections::HashMap;
use std::sync::Arc;

use hodei_policies::build_schema::error::BuildSchemaError;
use hodei_policies::build_schema::ports::SchemaStoragePort;
use hodei_policies::evaluate_policies::dto::{
    AuthorizationRequest, EvaluatePoliciesCommand, EvaluationDecision, EvaluationMode,
};
use hodei_policies::evaluate_policies::factories::create_evaluate_policies_use_case;
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
use kernel::{AttributeName, AttributeValue, HodeiEntity, Hrn};

/// A schema storage without any schema stored.
///
/// The evaluations run in `NoSchema` mode, so the storage is never consulted;
/// it exists only because the use case depends on the port.
struct EmptySchemaStorage;

#[async_trait::async_trait]
impl SchemaStoragePort for EmptySchemaStorage {
    async fn save_schema(
        &self,
        _schema_json: String,
        _version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        Err(BuildSchemaError::SchemaStorageError(
            "this storage is read-only".to_string(),
        ))
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
        Ok(false)
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        Ok(Vec::new())
    }
}

/// An entity identified by its HRN, without attributes.
#[derive(Debug)]
struct HrnEntity {
    hrn: Hrn,
}

impl HrnEntity {
    fn new(hrn: Hrn) -> Self {
        Self { hrn }
    }
}

impl HodeiEntity for HrnEntity {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        HashMap::new()
    }
}

/// Builds a policy set whose identifiers are the position of each policy.
fn policy_set(policies: &[(&str, &str)]) -> HodeiPolicySet {
    let policies = policies
        .iter()
        .map(|(id, content)| HodeiPolicy::new(PolicyId::new(*id), (*content).to_string()))
        .collect();

    HodeiPolicySet::new(policies)
}

/// Evaluates one request over the given policies.
async fn evaluate(
    port: &Arc<dyn EvaluatePoliciesPort>,
    policies: &[(&str, &str)],
    principal: &HrnEntity,
    resource: &HrnEntity,
) -> EvaluationDecision {
    let policy_set = policy_set(policies);
    let entities: [&dyn HodeiEntity; 2] = [principal, resource];

    let command = EvaluatePoliciesCommand {
        request: AuthorizationRequest {
            principal,
            action: "Read",
            resource,
            context: None,
        },
        policies: &policy_set,
        entities: &entities,
        schema_version: None,
        evaluation_mode: EvaluationMode::NoSchema,
    };

    port.evaluate(command)
        .await
        .expect("the evaluation must not fail")
}

/// The principal of account A and the homonym of account B.
fn alice_in(account: &str) -> HrnEntity {
    HrnEntity::new(Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        account.to_string(),
        "User".to_string(),
        "alice".to_string(),
    ))
}

/// The resource of every request.
fn bob_in(account: &str) -> HrnEntity {
    HrnEntity::new(Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        account.to_string(),
        "User".to_string(),
        "bob".to_string(),
    ))
}

/// A1 — Dos evaluaciones concurrentes con políticas opuestas no se contaminan.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a1_concurrent_evaluations_do_not_contaminate_each_other() {
    let port = create_evaluate_policies_use_case(Arc::new(EmptySchemaStorage));

    let mut handles = Vec::new();
    for index in 0..16usize {
        let port = Arc::clone(&port);
        handles.push(tokio::spawn(async move {
            let principal = alice_in("account-a");
            let resource = bob_in("account-a");

            if index % 2 == 0 {
                let decision = evaluate(
                    &port,
                    &[("permit_all", "permit(principal, action, resource);")],
                    &principal,
                    &resource,
                )
                .await;
                (index, decision.decision, decision.determining_policies)
            } else {
                let decision = evaluate(
                    &port,
                    &[("forbid_all", "forbid(principal, action, resource);")],
                    &principal,
                    &resource,
                )
                .await;
                (index, decision.decision, decision.determining_policies)
            }
        }));
    }

    for handle in handles {
        let (index, decision, determining) = handle.await.expect("the task must not panic");

        if index % 2 == 0 {
            assert_eq!(
                decision,
                hodei_policies::evaluate_policies::dto::Decision::Allow,
                "request {index} was contaminated"
            );
            assert_eq!(
                determining,
                vec!["permit_all".to_string()],
                "request {index} named a foreign policy"
            );
        } else {
            assert_eq!(
                decision,
                hodei_policies::evaluate_policies::dto::Decision::Deny,
                "request {index} was contaminated"
            );
            assert_eq!(
                determining,
                vec!["forbid_all".to_string()],
                "request {index} named a foreign policy"
            );
        }
    }
}

/// A2 — Una política que nombra una cuenta no alcanza a su homónimo.
#[tokio::test]
async fn a2_a_policy_scoped_to_one_account_does_not_reach_another_account() {
    let port = create_evaluate_policies_use_case(Arc::new(EmptySchemaStorage));

    let policy = (
        "only_account_a",
        "permit(principal, action, resource) when { principal == Iam::User::\"hrn:aws:iam::account-a:User/alice\" };",
    );

    let alice_a = alice_in("account-a");
    let bob_a = bob_in("account-a");
    let decision = evaluate(&port, &[policy], &alice_a, &bob_a).await;
    assert_eq!(
        decision.decision,
        hodei_policies::evaluate_policies::dto::Decision::Allow
    );

    // The homonym of account B is a different entity: the same policy does
    // not grant it. The resource is the principal of account A, so the entity
    // the policy mentions exists in the request and the denial comes from the
    // policy, not from an evaluation error.
    let alice_b = alice_in("account-b");
    let alice_a_as_resource = alice_in("account-a");
    let decision = evaluate(&port, &[policy], &alice_b, &alice_a_as_resource).await;
    assert_eq!(
        decision.decision,
        hodei_policies::evaluate_policies::dto::Decision::Deny,
        "a policy scoped to account-a must not grant account-b"
    );
    assert!(
        !decision
            .diagnostics
            .iter()
            .any(|d| d.level == hodei_policies::evaluate_policies::dto::DiagnosticLevel::Error),
        "the denial must come from the policy, not from an evaluation error: {:?}",
        decision.diagnostics
    );
}

/// A3 — Sin políticas, el default es deny.
#[tokio::test]
async fn a3_without_policies_the_default_is_deny() {
    let port = create_evaluate_policies_use_case(Arc::new(EmptySchemaStorage));

    let alice = alice_in("account-a");
    let bob = bob_in("account-a");
    let decision = evaluate(&port, &[], &alice, &bob).await;

    assert_eq!(
        decision.decision,
        hodei_policies::evaluate_policies::dto::Decision::Deny
    );
}
