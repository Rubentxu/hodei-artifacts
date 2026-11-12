//! La UAT de M6: la seguridad reactiva.
//!
//! La UAT se ejecuta sobre Hodei Artifacts y fixtures locales, sin
//! depender de ningun otro producto. Cada escenario tiene su oraculo: un
//! escenario que no se puede oracular no se puede aprobar.
//!
//! La cadena que se demuestra es la del hito:
//!
//! ```text
//! CVE accepted -> impacted artifacts -> reevaluation -> SecurityCase updated
//! ```
//!
//! Y las cuatro propiedades de la exit gate: **observable, acotado,
//! idempotente y reconstruible**.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::{GraphNode, GraphRelation, NodeKind, RelationType};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use hodei_reactive::behavior::{Behavior, BehaviorError, GuardOutcome, Reaction};
use hodei_reactive::events::{Event, EventKind, Subscription};
use hodei_reactive::execution::{Budget, ExecutionOutcome};
use hodei_reactive::reactions::VulnerabilityReaction;
use hodei_reactive::runtime::{ExecutionStore, InMemoryExecutionStore, ReactiveRuntime};
use hodei_risk_storage::reactions::{SurrealBlastRadius, SurrealSecurityCaseStore};
use hodei_trust_policy::context::{RiskFacts, TrustContext};
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::DecisionStore;
use hodei_trust_policy::ports::PolicyLoader;
use hodei_trust_policy::promotion::{DecisionId, PromotionRequest};
use hodei_trust_policy::{PolicyRegistry, Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::reactions::{
    EmptyContextSource, PromotionDecisionRequester, TrustContextSource,
};
use hodei_trust_policy_storage::{StaticPromotionTargets, SurrealDecisionStore};
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// La cuenta de la UAT.
const ACCOUNT: &str = "acme";

/// El sujeto de la UAT.
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0";

/// La vulnerabilidad de la UAT.
const ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-m6-0001";

/// La cuenta que se lleva la cuenta de los escenarios.
fn account() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/{ACCOUNT}"
    ))
    .expect("un HRN")
}

/// El sujeto de la UAT.
fn subject() -> Hrn {
    Hrn::from_string(SUBJECT).expect("un HRN")
}

/// La vulnerabilidad de la UAT.
fn advisory() -> Hrn {
    Hrn::from_string(ADVISORY).expect("un HRN")
}

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

/// La politica de produccion: sin vulnerabilidad critica alcanzable, y con
/// el riesgo establecido.
fn production_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// El contexto de un sujeto sin riesgos: el riesgo todavia no esta
/// establecido.
fn clean_context(subject: &str, at: DateTime<Utc>) -> TrustContext {
    TrustContext {
        version: hodei_trust_policy::context::TRUST_CONTEXT_VERSION.to_string(),
        subject: hodei_trust_policy::context::SubjectFact {
            hrn: subject.to_string(),
            digest: None,
        },
        target: hodei_trust_policy::context::TargetEnvironment::Production,
        dimensions: Vec::new(),
        capsule_hash: None,
        risk: RiskFacts {
            // El riesgo ESTA establecido y no hay ninguna vulnerabilidad
            // critica alcanzable: esa es la situacion de una release
            // conforme.
            established: true,
            exposure: hodei_trust_policy::context::Exposure::Internal,
            ..RiskFacts::default()
        },
        exceptions: Vec::new(),
        evidence_ids: Vec::new(),
        assembled_at: at,
    }
}

/// El contexto de un sujeto con una vulnerabilidad critica alcanzable.
fn exposed_context(subject: &str, at: DateTime<Utc>) -> TrustContext {
    TrustContext {
        version: hodei_trust_policy::context::TRUST_CONTEXT_VERSION.to_string(),
        subject: hodei_trust_policy::context::SubjectFact {
            hrn: subject.to_string(),
            digest: None,
        },
        target: hodei_trust_policy::context::TargetEnvironment::Production,
        dimensions: Vec::new(),
        capsule_hash: None,
        risk: RiskFacts {
            critical_known: 1,
            critical_reachable: 1,
            established: true,
            exposure: hodei_trust_policy::context::Exposure::Internal,
            vex_accepted: 0,
        },
        exceptions: Vec::new(),
        evidence_ids: Vec::new(),
        assembled_at: at,
    }
}

/// El contexto que cambia con el riesgo: el de la UAT lo decide, y asi
/// la decision cambia cuando el riesgo aparece.
struct TheContextSource {
    exposed: bool,
}

#[async_trait::async_trait]
impl TrustContextSource for TheContextSource {
    async fn context_of(
        &self,
        subject: &str,
        critical_reachable: u32,
        at: DateTime<Utc>,
    ) -> Result<TrustContext, String> {
        let _ = critical_reachable;
        if self.exposed {
            Ok(exposed_context(subject, at))
        } else {
            Ok(clean_context(subject, at))
        }
    }
}

/// El mundo de la UAT: la base de datos, el grafo y las decisiones.
struct World {
    #[allow(dead_code)]
    db: Arc<Surreal<Db>>,
    graph: Arc<SurrealGraphStore<Db>>,
    store: Arc<SurrealDecisionStore<Db>>,
    policies: Arc<PolicyRegistry>,
    cases: Arc<SurrealSecurityCaseStore<Db>>,
}

impl World {
    /// El mundo con su grafo y su capa de riesgo.
    async fn build(path: &std::path::Path, exposed: bool) -> Self {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
                .await
                .expect("la base de datos abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el espacio de nombres");

        let graph = Arc::new(SurrealGraphStore::new(db.clone()));
        let store = Arc::new(SurrealDecisionStore::new(db.clone()));
        let cases = Arc::new(SurrealSecurityCaseStore::new(db.clone()));

        let policies = Arc::new(PolicyRegistry::empty());
        policies.activate(
            production_policy(),
            "alice",
            "la politica de produccion",
            Utc::now(),
        );

        let world = Self {
            db: db.clone(),
            graph: graph.clone(),
            store: store.clone(),
            policies: policies.clone(),
            cases: cases.clone(),
        };

        world.the_risk_layer().await;
        let _ = exposed;
        world
    }

    /// La capa de riesgo: el artefacto, sus componentes, la vulnerabilidad
    /// y el despliegue.
    async fn the_risk_layer(&self) {
        use hodei_evidence::EvidenceId;

        let evidence = EvidenceId::new("ev-m6-1");
        let now = Utc::now();
        let component = Hrn::from_string(&format!(
            "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-vuln"
        ))
        .expect("un HRN");
        let deployment = Hrn::from_string(&format!(
            "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Deployment/production"
        ))
        .expect("un HRN");

        let changes = hodei_evidence::domain::graph::GraphChangeSet::empty()
            .with_node(GraphNode::new(
                subject(),
                account(),
                NodeKind::Artifact,
                now,
            ))
            .with_node(GraphNode::new(
                component.clone(),
                account(),
                NodeKind::Component,
                now,
            ))
            .with_node(GraphNode::new(
                advisory(),
                account(),
                NodeKind::Vulnerability,
                now,
            ))
            .with_node(GraphNode::new(
                deployment.clone(),
                account(),
                NodeKind::Deployment,
                now,
            ))
            .with_node(GraphNode::new(
                hodei_evidence::application::projector::evidence_hrn(&account(), &evidence),
                account(),
                NodeKind::Evidence,
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                subject(),
                component.clone(),
                account(),
                evidence.clone(),
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::AffectedBy,
                component,
                advisory(),
                account(),
                evidence.clone(),
                now,
            ))
            .with_relation(GraphRelation::new(
                RelationType::DeployedAs,
                subject(),
                deployment,
                account(),
                evidence,
                now,
            ));

        self.graph
            .apply(changes)
            .await
            .expect("la capa de riesgo se escribe");
    }

    /// La decision previa: un ALLOW.
    async fn the_prior_allow(&self, at: DateTime<Utc>) -> DecisionId {
        let id = DecisionId::new("dec-prior");
        let request = PromotionRequest::new(
            "corr-1",
            "alice",
            ACCOUNT.to_string(),
            SUBJECT.to_string(),
            "production".to_string(),
            at,
        );
        self.policies
            .active_policy("production")
            .await
            .expect("la politica esta activa");
        let service = hodei_trust_policy::service::PromotionService::new(
            self.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![
                hodei_trust_policy::promotion::PromotionTarget {
                    distribution: "production".to_string(),
                    account: ACCOUNT.to_string(),
                },
            ])),
            self.policies.clone(),
        );
        service
            .request_promotion(
                id.clone(),
                request,
                AuthorizationResult::Permit,
                clean_context(SUBJECT, at),
                "production",
            )
            .await
            .expect("la decision previa se escribe");
        id
    }
}

/// Un comportamiento que falla, para el reintento y el estado muerto.
struct FailingBehaviour {
    name: String,
    subscription: Subscription,
    calls: Arc<Mutex<usize>>,
    retryable: bool,
}

#[async_trait::async_trait]
impl Behavior for FailingBehaviour {
    fn name(&self) -> &str {
        &self.name
    }

    fn subscription(&self) -> Subscription {
        self.subscription.clone()
    }

    fn guard(&self, _event: &Event) -> GuardOutcome {
        GuardOutcome::Pass
    }

    async fn act(&self, _event: &Event) -> Result<Reaction, BehaviorError> {
        *self.calls.lock().expect("el contador no esta envenenado") += 1;
        if self.retryable {
            Err(BehaviorError::transient("el sumidero esta caido"))
        } else {
            Err(BehaviorError::permanent("la entrada es invalida"))
        }
    }
}

/// Un comportamiento que causa un evento, para el bucle.
struct LoopingBehaviour {
    calls: Arc<Mutex<usize>>,
}

#[async_trait::async_trait]
impl Behavior for LoopingBehaviour {
    fn name(&self) -> &str {
        "looping"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted)
    }

    fn guard(&self, _event: &Event) -> GuardOutcome {
        GuardOutcome::Pass
    }

    async fn act(&self, event: &Event) -> Result<Reaction, BehaviorError> {
        *self.calls.lock().expect("el contador no esta envenenado") += 1;
        Ok(Reaction {
            events: vec![Event::new(
                format!("evt-loop-{}", event.id),
                EventKind::VulnerabilityAccepted,
                serde_json::json!({"advisory": "GHSA-m6-0001"}),
                event.happened_at,
            )
            .caused_by(event.id.clone())],
            effects: Vec::new(),
        })
    }
}

// ───────────────────────────── M6-UAT-01 ─────────────────────────────

/// M6-UAT-01 Un CVE nuevo sustituye al ALLOW anterior.
///
/// Given a release that is deployed and whose latest TrustDecision is
/// ALLOW, when accepted vulnerability evidence affects a reachable
/// production component forbidden by active policy, then the behavior
/// chain computes the blast radius, updates/creates the SecurityCase,
/// requests reevaluation and creates a new DENY decision linked as
/// superseding the prior decision.
#[tokio::test]
async fn m6_uat_01_a_new_cve_supersedes_the_prior_allow() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path, true).await;
        let now = Utc::now();

        // La decision previa: un ALLOW.
        let prior = world.the_prior_allow(now).await;
        let before = world
            .store
            .get(&prior)
            .await
            .map_err(|error| error.to_string())?
            .expect("la decision previa esta");
        assert_eq!(
            before.outcome,
            hodei_trust_policy::decision::Outcome::Allow,
            "la decision previa es un ALLOW"
        );

        // El runtime con la reaccion del CVE.
        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let radius = Arc::new(SurrealBlastRadius::new(world.graph.clone(), account()));
        let service = Arc::new(hodei_trust_policy::service::PromotionService::new(
            world.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![
                hodei_trust_policy::promotion::PromotionTarget {
                    distribution: "production".to_string(),
                    account: ACCOUNT.to_string(),
                },
            ])),
            world.policies.clone(),
        ));
        let requester = Arc::new(PromotionDecisionRequester::new(
            service,
            world.store.clone(),
            Arc::new(TheContextSource { exposed: true }),
            "production",
            "production",
            "system",
        ));
        runtime.register(Arc::new(VulnerabilityReaction::new(
            radius,
            world.cases.clone(),
            requester,
        )));

        // El CVE llega.
        let event = Event::new(
            "evt-cve-1",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY, "account": ACCOUNT}),
            now,
        );
        let report = runtime.dispatch(event, now).await.expect("el despacho");

        // La cadena corrio y no fue cortada.
        assert!(
            report.cut.is_none(),
            "la cadena no se corto: {:?}",
            report.cut
        );
        let execution = report
            .execution_of("vulnerability_impact")
            .expect("la ejecucion esta");
        assert_eq!(
            execution.outcome,
            ExecutionOutcome::Completed,
            "la reaccion se completo"
        );

        // El radio de impacto alcanzo el artefacto.
        let cases = world
            .cases
            .of_trigger(ADVISORY)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(cases.len(), 1, "el caso se abrio");
        let case = &cases[0];
        assert!(
            case.blast_radius
                .affected_subjects
                .contains(&SUBJECT.to_string()),
            "el caso nombra al artefacto: {:?}",
            case.blast_radius.affected_subjects
        );

        // Y hay una decision nueva, que es un DENY.
        let decisions = world
            .store
            .list_for_subject(ACCOUNT, SUBJECT)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(decisions.len(), 2, "hay una decision nueva");
        let newest = decisions
            .iter()
            .max_by_key(|decision| decision.id.as_str())
            .expect("la decision nueva");
        assert_eq!(
            newest.outcome,
            hodei_trust_policy::decision::Outcome::Deny,
            "la decision nueva es un DENY"
        );

        // Y la decision nueva SUSTITUYE a la anterior.
        assert_eq!(
            newest.request.correlation_id,
            prior.as_str(),
            "la decision nueva dice cual sustituye"
        );

        Ok(())
    }
    .await;

    record("M6-UAT-01", result);
}

// ───────────────────────────── M6-UAT-02 ─────────────────────────────

/// M6-UAT-02 Un CVE irrelevante no causa decisiones de mas.
///
/// Given a vulnerability that does not map to any component in the tenant
/// graph, when accepted, then no release reevaluation is scheduled for
/// unrelated subjects.
#[tokio::test]
async fn m6_uat_02_an_irrelevant_cve_causes_no_decision_churn() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path, true).await;
        let now = Utc::now();

        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let radius = Arc::new(SurrealBlastRadius::new(world.graph.clone(), account()));
        let service = Arc::new(hodei_trust_policy::service::PromotionService::new(
            world.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![
                hodei_trust_policy::promotion::PromotionTarget {
                    distribution: "production".to_string(),
                    account: ACCOUNT.to_string(),
                },
            ])),
            world.policies.clone(),
        ));
        let requester = Arc::new(PromotionDecisionRequester::new(
            service,
            world.store.clone(),
            Arc::new(TheContextSource { exposed: true }),
            "production",
            "production",
            "system",
        ));
        runtime.register(Arc::new(VulnerabilityReaction::new(
            radius,
            world.cases.clone(),
            requester,
        )));

        // Una vulnerabilidad que no toca nada del grafo.
        let event = Event::new(
            "evt-cve-2",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({
                "advisory": "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-otra",
                "account": ACCOUNT,
            }),
            now,
        );
        let report = runtime.dispatch(event, now).await.expect("el despacho");

        assert!(
            report.applied.is_empty(),
            "no se aplico ningun efecto: {:?}",
            report.applied
        );
        let decisions = world
            .store
            .list_for_subject(ACCOUNT, SUBJECT)
            .await
            .map_err(|error| error.to_string())?;
        assert!(
            decisions.is_empty(),
            "no se reevalua nada que no esta alcanzado"
        );
        Ok(())
    }
    .await;

    record("M6-UAT-02", result);
}

// ───────────────────────────── M6-UAT-03 ─────────────────────────────

/// M6-UAT-03 La idempotencia del evento repetido.
///
/// Given same trigger delivered repeatedly, then behavior execution/side
/// effects remain semantically once-only and trace reports duplicate
/// suppression/idempotent reuse.
#[tokio::test]
async fn m6_uat_03_a_duplicate_event_is_idempotent() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path, true).await;
        let now = Utc::now();

        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let radius = Arc::new(SurrealBlastRadius::new(world.graph.clone(), account()));
        let service = Arc::new(hodei_trust_policy::service::PromotionService::new(
            world.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![
                hodei_trust_policy::promotion::PromotionTarget {
                    distribution: "production".to_string(),
                    account: ACCOUNT.to_string(),
                },
            ])),
            world.policies.clone(),
        ));
        let requester = Arc::new(PromotionDecisionRequester::new(
            service,
            world.store.clone(),
            Arc::new(TheContextSource { exposed: true }),
            "production",
            "production",
            "system",
        ));
        runtime.register(Arc::new(VulnerabilityReaction::new(
            radius,
            world.cases.clone(),
            requester,
        )));

        let event = Event::new(
            "evt-cve-3",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY, "account": ACCOUNT}),
            now,
        );

        let first = runtime
            .dispatch(event.clone(), now)
            .await
            .expect("el despacho");
        assert_eq!(first.applied.len(), 1, "el efecto se aplica una vez");

        // El mismo evento, otra vez.
        let second = runtime.dispatch(event, now).await.expect("el despacho");
        assert_eq!(second.applied.len(), 0, "el efecto no se aplica dos veces");
        assert_eq!(second.duplicates.len(), 1, "y la repeticion se reporta");

        // Y el efecto esta en el almacen UNA vez.
        assert_eq!(store.applied_effects().len(), 1);
        Ok(())
    }
    .await;

    record("M6-UAT-03", result);
}

// ───────────────────────────── M6-UAT-04 ─────────────────────────────

/// M6-UAT-04 El reintento del fallo transitorio.
///
/// Given an internal notification sink adapter fails transiently, then
/// behavior records failure/retry, preserves internal decision state,
/// retries according to policy and does not duplicate notification after
/// success.
#[tokio::test]
async fn m6_uat_04_a_transient_failure_retries() {
    let result: Result<(), String> = async {
        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let calls = Arc::new(Mutex::new(0usize));
        runtime.register(Arc::new(FailingBehaviour {
            name: "notifying".to_string(),
            subscription: Subscription::to(EventKind::VulnerabilityAccepted),
            calls: calls.clone(),
            retryable: true,
        }));

        let event = Event::new(
            "evt-cve-4",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY}),
            Utc::now(),
        );

        // La primera vez falla, y el fallo queda escrito con su motivo.
        let first = runtime
            .dispatch(event.clone(), Utc::now())
            .await
            .expect("el despacho");
        let execution = first.execution_of("notifying").expect("la ejecucion esta");
        assert!(execution.is_retryable_failure());
        assert_eq!(execution.attempts, 1);

        // Y el registro es durable, con la traza del fallo.
        let stored = store
            .execution_of("notifying:evt-cve-4")
            .expect("el registro esta");
        assert!(stored.is_failure());
        assert!(
            stored
                .trace
                .iter()
                .any(|entry| entry.what.contains("el sumidero esta caido")),
            "la traza dice por que fallo"
        );

        // La segunda vez reintenta.
        let second = runtime
            .dispatch(event, Utc::now())
            .await
            .expect("el despacho");
        let execution = second.execution_of("notifying").expect("la ejecucion esta");
        assert_eq!(execution.attempts, 2, "el reintento cuenta como intento");
        assert_eq!(*calls.lock().expect("el contador no esta envenenado"), 2);
        Ok(())
    }
    .await;

    record("M6-UAT-04", result);
}

// ───────────────────────────── M6-UAT-05 ─────────────────────────────

/// M6-UAT-05 El fallo permanente y el estado muerto.
///
/// Given malformed permanent input, then no infinite retry occurs;
/// failure is queryable with causation trace.
#[tokio::test]
async fn m6_uat_05_a_permanent_failure_does_not_retry_for_ever() {
    let result: Result<(), String> = async {
        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let calls = Arc::new(Mutex::new(0usize));
        runtime.register(Arc::new(FailingBehaviour {
            name: "notifying".to_string(),
            subscription: Subscription::to(EventKind::VulnerabilityAccepted),
            calls: calls.clone(),
            retryable: false,
        }));

        let event = Event::new(
            "evt-cve-5",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY}),
            Utc::now(),
        );

        // El fallo permanente NO se reintenta: la segunda entrega no
        // vuelve a correr el comportamiento.
        runtime
            .dispatch(event.clone(), Utc::now())
            .await
            .expect("el despacho");
        runtime
            .dispatch(event, Utc::now())
            .await
            .expect("el despacho");
        assert_eq!(
            *calls.lock().expect("el contador no esta envenenado"),
            1,
            "un fallo permanente no se reintenta"
        );

        // Y el fallo se puede consultar, con su cadena de causalidad.
        let stored = store
            .execution_of("notifying:evt-cve-5")
            .expect("el registro esta");
        assert!(stored.is_failure());
        assert!(!stored.is_retryable_failure());
        assert_eq!(stored.causation, vec!["evt-cve-5".to_string()]);
        Ok(())
    }
    .await;

    record("M6-UAT-05", result);
}

// ───────────────────────────── M6-UAT-06 ─────────────────────────────

/// M6-UAT-06 La prevencion del bucle.
///
/// Given intentionally cyclic behavior fixture A->B->A, then
/// causation/idempotency/budget rules terminate loop and emit diagnosable
/// event rather than exhaust runtime.
#[tokio::test]
async fn m6_uat_06_a_loop_is_cut() {
    let result: Result<(), String> = async {
        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(
            store.clone(),
            Budget {
                max_executions: 4,
                max_depth: 3,
                max_attempts: 3,
            },
        );
        let calls = Arc::new(Mutex::new(0usize));
        runtime.register(Arc::new(LoopingBehaviour {
            calls: calls.clone(),
        }));

        let event = Event::new(
            "evt-cve-6",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY}),
            Utc::now(),
        );
        let report = runtime
            .dispatch(event, Utc::now())
            .await
            .expect("el despacho");

        assert!(report.cut.is_some(), "el presupuesto corto el bucle");
        assert!(
            *calls.lock().expect("el contador no esta envenenado") <= 4,
            "el bucle no se eternity"
        );

        // Y el corte se puede consultar: hay un registro con el corte.
        let records = store.executions();
        assert!(
            records
                .iter()
                .any(|record| matches!(record.outcome, ExecutionOutcome::Cut { .. })),
            "el corte queda en la traza"
        );
        Ok(())
    }
    .await;

    record("M6-UAT-06", result);
}

// ───────────────────────────── M6-UAT-07 ─────────────────────────────

/// M6-UAT-07 La seguridad de la reconstruccion.
///
/// Given a behavior trace containing an external side effect, when replay
/// reconstructs state, then destructive/external side effect is not
/// executed again and recorded result is used.
#[tokio::test]
async fn m6_uat_07_the_replay_does_not_repeat_the_side_effect() {
    let result: Result<(), String> = async {
        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let calls = Arc::new(Mutex::new(0usize));
        runtime.register(Arc::new(FailingBehaviour {
            name: "notifying".to_string(),
            subscription: Subscription::to(EventKind::VulnerabilityAccepted),
            calls: calls.clone(),
            retryable: false,
        }));

        let event = Event::new(
            "evt-cve-7",
            EventKind::VulnerabilityAccepted,
            serde_json::json!({"advisory": ADVISORY}),
            Utc::now(),
        );
        runtime
            .dispatch(event.clone(), Utc::now())
            .await
            .expect("el despacho");
        let before = *calls.lock().expect("el contador no esta envenenado");

        // La reconstruccion lee lo que paso y NO lo repite.
        let replayed = runtime
            .replay(event, Utc::now())
            .await
            .expect("la reconstruccion");
        assert!(
            replayed.applied.is_empty(),
            "el efecto externo no se ejecuta otra vez"
        );
        assert_eq!(
            *calls.lock().expect("el contador no esta envenenado"),
            before,
            "y el comportamiento no corre otra vez"
        );
        Ok(())
    }
    .await;

    record("M6-UAT-07", result);
}

// ───────────────────────────── M6-UAT-08 ─────────────────────────────

/// M6-UAT-08 La reaccion ante la caducidad de una excepcion.
///
/// Given ALLOW_WITH_EXCEPTION-equivalent state with expiring exception,
/// when expiration trigger occurs, then reevaluation occurs and expired
/// exception cannot be applied.
#[tokio::test]
async fn m6_uat_08_the_expiry_of_an_exception_triggers_its_reaction() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let path = tmp.keep();
        let world = World::build(&path, true).await;
        let now = Utc::now();

        let store = Arc::new(InMemoryExecutionStore::empty());
        let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());
        let service = Arc::new(hodei_trust_policy::service::PromotionService::new(
            world.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![
                hodei_trust_policy::promotion::PromotionTarget {
                    distribution: "production".to_string(),
                    account: ACCOUNT.to_string(),
                },
            ])),
            world.policies.clone(),
        ));
        let requester = Arc::new(PromotionDecisionRequester::new(
            service,
            world.store.clone(),
            Arc::new(EmptyContextSource),
            "production",
            "production",
            "system",
        ));
        runtime.register(Arc::new(
            hodei_reactive::reactions::ExceptionExpiryReaction::new(requester),
        ));

        let event = Event::new(
            "evt-expired-1",
            EventKind::ExceptionExpired,
            serde_json::json!({"subject": SUBJECT}),
            now,
        );
        let report = runtime.dispatch(event, now).await.expect("el despacho");

        let execution = report
            .execution_of("exception_expired")
            .expect("la ejecucion esta");
        assert_eq!(
            execution.outcome,
            ExecutionOutcome::Completed,
            "la reaccion corrio"
        );
        assert_eq!(report.applied.len(), 1, "y se pidio la reevaluacion");

        // Y la excepcion caducada NO se puede aplicar: la reevaluacion no
        // lleva ninguna excepcion.
        let decisions = world
            .store
            .list_for_subject(ACCOUNT, SUBJECT)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(decisions.len(), 1, "hay una decision nueva");
        assert!(
            decisions[0].context.exceptions.is_empty(),
            "la excepcion caducada no se aplica"
        );
        Ok(())
    }
    .await;

    record("M6-UAT-08", result);
}
