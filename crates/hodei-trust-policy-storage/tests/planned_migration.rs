//! La migracion de las reacciones reales al ejecutor de intents.
//!
//! La cadena es la del hito, por la ruta que dice antes de tocar:
//!
//! ```text
//! CVE accepted -> plan (radio de impacto) -> outbox -> SecurityCase +
//!                outbox -> reevaluation -> decision nueva
//! ```
//!
//! Todo corre sobre el SurrealDB real de Artifacts, con los adaptadores
//! de siempre --SurrealBlastRadius, SurrealSecurityCaseStore,
//! PromotionDecisionRequester--, un unico almacen y ningun broker nuevo.
//! Lo que cambia es QUIEN toca: el plan lee y dice, y el ejecutor toca
//! despues, con recibo.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::{GraphNode, GraphRelation, NodeKind, RelationType};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence::EvidenceId;
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use hodei_reactive::events::{Event, EventKind};
use hodei_reactive::execution::Budget;
use hodei_reactive::intent::IntentOutbox;
use hodei_reactive::intent::{IntentExecutor, Receipt};
use hodei_reactive::reactions::{
    DecisionRequester, PlannedExceptionExpiryReaction, PlannedVulnerabilityReaction,
};
use hodei_reactive::runtime::{ExecutionStore, ReactiveRuntime};
use hodei_risk_storage::reactions::{SurrealBlastRadius, SurrealSecurityCaseStore};
use hodei_trust_policy::context::TrustContext;
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::{DecisionStore, PolicyLoader};
use hodei_trust_policy::promotion::{DecisionId, PromotionRequest, PromotionTarget};
use hodei_trust_policy::service::PromotionService;
use hodei_trust_policy::{PolicyRegistry, Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::intents::SurrealIntentStore;
use hodei_trust_policy_storage::reactions::{EmptyContextSource, PromotionDecisionRequester};
use hodei_trust_policy_storage::{StaticPromotionTargets, SurrealDecisionStore};
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// La cuenta de la migracion.
const ACCOUNT: &str = "acme";

/// El sujeto de la migracion.
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0";

/// La vulnerabilidad de la migracion.
const ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-mig-0001";

/// La vulnerabilidad que no toca nada.
const ADVISORY_IRRELEVANT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-mig-none";

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
        risk: hodei_trust_policy::context::RiskFacts {
            established: true,
            exposure: hodei_trust_policy::context::Exposure::Internal,
            ..hodei_trust_policy::context::RiskFacts::default()
        },
        exceptions: Vec::new(),
        evidence_ids: Vec::new(),
        assembled_at: at,
    }
}

fn account() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/{ACCOUNT}"
    ))
    .expect("un HRN")
}

fn subject() -> Hrn {
    Hrn::from_string(SUBJECT).expect("un HRN")
}

fn advisory() -> Hrn {
    Hrn::from_string(ADVISORY).expect("un HRN")
}

/// El evento de una CVE aceptada.
fn the_cve_event(id: &str) -> Event {
    Event::new(
        id,
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": ADVISORY, "account": ACCOUNT}),
        Utc::now(),
    )
}

/// El mundo de la migracion: la base de datos, el grafo, las decisiones,
/// los casos y el outbox, TODOS sobre la misma base.
struct World {
    db: Arc<Surreal<Db>>,
    graph: Arc<SurrealGraphStore<Db>>,
    store: Arc<SurrealDecisionStore<Db>>,
    policies: Arc<PolicyRegistry>,
    cases: Arc<SurrealSecurityCaseStore<Db>>,
    radius: Arc<SurrealBlastRadius<Db>>,
    intents: Arc<SurrealIntentStore<Db>>,
}

impl World {
    async fn build(path: &std::path::Path) -> Self {
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
        let radius = Arc::new(SurrealBlastRadius::new(graph.clone(), account()));
        let intents = Arc::new(SurrealIntentStore::new(db.clone()));

        let policies = Arc::new(PolicyRegistry::empty());
        policies.activate(
            production_policy(),
            "alice",
            "la politica de produccion",
            Utc::now(),
        );

        let world = Self {
            db,
            graph,
            store,
            policies,
            cases,
            radius,
            intents,
        };
        world.the_risk_layer().await;
        world
    }

    /// La capa de riesgo: el artefacto, su componente vulnerable, la
    /// vulnerabilidad y el despliegue.
    async fn the_risk_layer(&self) {
        let evidence = EvidenceId::new("ev-mig-1");
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

    /// La decision previa: un ALLOW que la nueva decidida va a sustituir.
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
        let service = PromotionService::new(
            self.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
                distribution: "production".to_string(),
                account: ACCOUNT.to_string(),
            }])),
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

    /// El pidiente de decisiones real, sobre el servicio de promocion.
    fn the_requester(&self) -> Arc<PromotionDecisionRequester> {
        let service = Arc::new(PromotionService::new(
            self.store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
                distribution: "production".to_string(),
                account: ACCOUNT.to_string(),
            }])),
            self.policies.clone(),
        ));
        Arc::new(PromotionDecisionRequester::new(
            service,
            self.store.clone(),
            Arc::new(EmptyContextSource),
            "production",
            "production",
            "system",
        ))
    }

    /// El runtime planificado sobre SU outbox y SU registro de
    /// ejecuciones, con el ejecutor que se le pase o el real.
    async fn the_runtime(&self, executor: Option<Arc<dyn IntentExecutor>>) -> ReactiveRuntime {
        let store: Arc<dyn ExecutionStore> = self.intents.clone();
        let outbox: Arc<dyn IntentOutbox> = self.intents.clone();
        let mut runtime = ReactiveRuntime::new(store, Budget::default()).with_tenant(ACCOUNT);
        runtime.with_outbox(outbox);
        let executor = executor.unwrap_or_else(|| {
            Arc::new(hodei_reactive::reactions::DomainIntentExecutor::new(
                ACCOUNT,
                self.cases.clone() as Arc<dyn hodei_reactive::reactions::SecurityCaseSink>,
                self.the_requester() as Arc<dyn DecisionRequester>,
            ))
        });
        runtime.register_planned(
            Arc::new(PlannedVulnerabilityReaction::new(self.radius.clone())),
            executor.clone(),
        );
        runtime.register_planned(Arc::new(PlannedExceptionExpiryReaction::new()), executor);
        runtime
    }
}

/// El ejecutor que responde UNKNOWN la primera vez que toca una
/// decision, y delega despues: el recibo sincero de un mundo que no
/// contesta.
struct UnknownDecisionOnce {
    inner: Arc<dyn IntentExecutor>,
    seen: Mutex<HashSet<String>>,
}

#[async_trait::async_trait]
impl IntentExecutor for UnknownDecisionOnce {
    async fn apply(&self, intent: &hodei_reactive::intent::Intent) -> Receipt {
        if intent.kind == "decision_reevaluation" {
            let mut seen = self.seen.lock().expect("el conjunto no esta envenenado");
            if seen.insert(intent.identity.clone()) {
                return Receipt::Unknown {
                    reason: "the answer never came".to_string(),
                };
            }
        }
        self.inner.apply(intent).await
    }
}

/// El ejecutor caido: todo lo que toca falla con reintento.
struct DownExecutor;

#[async_trait::async_trait]
impl IntentExecutor for DownExecutor {
    async fn apply(&self, _intent: &hodei_reactive::intent::Intent) -> Receipt {
        Receipt::Failed {
            reason: "the case store is down".to_string(),
            retryable: true,
        }
    }
}

/// AC1: la ruta CVE aceptada -> impacto -> caso -> decision corre por el
/// outbox, la decision nueva enlaza la que sustituye, y la reentrega no
/// crea nada de nuevo.
#[tokio::test]
async fn the_cve_route_runs_through_the_outbox_and_links_the_superseded_decision() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let world = World::build(tmp.path()).await;
    let now = Utc::now();

    let prior = world.the_prior_allow(now).await;
    let runtime = world.the_runtime(None).await;

    let report = runtime
        .dispatch_planned(the_cve_event("evt-cve-1"), now)
        .await
        .expect("el despacho planificado corre");

    // La ejecucion del plan se completo y no se corto.
    assert!(
        report.cut.is_none(),
        "la cadena no se corto: {:?}",
        report.cut
    );
    let execution = report
        .execution_of("vulnerability_impact")
        .expect("la ejecucion del plan esta");
    assert_eq!(
        execution.outcome,
        hodei_reactive::execution::ExecutionOutcome::Completed,
        "el plan se completo"
    );

    // El caso de seguridad existe, y nombra al artefacto.
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert_eq!(cases.len(), 1, "el caso se abrio UNA vez");
    assert!(
        cases[0]
            .blast_radius
            .affected_subjects
            .contains(&SUBJECT.to_string()),
        "el caso nombra al artefacto"
    );

    // La decision nueva existe y SUSTITUYE a la anterior.
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
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
    assert_eq!(
        newest.request.correlation_id,
        prior.as_str(),
        "la decision nueva dice cual sustituye"
    );

    // Y TODOS los intents del outbox quedaron aplicados: el estado vive
    // en la base, no en la memoria del proceso.
    let states = world.intents.states().await.expect("los estados se leen");
    assert!(
        !states.is_empty(),
        "el outbox guardo los intents de la ruta"
    );
    assert!(
        states.iter().all(|(_identity, state)| state == "applied"),
        "todos los intents quedaron aplicados: {states:?}"
    );

    // La reentrega del mismo evento: duplicado, y ni un caso ni una
    // decision mas.
    let again = runtime
        .dispatch_planned(the_cve_event("evt-cve-1"), now)
        .await
        .expect("la reentrega corre");
    assert!(!again.duplicates.is_empty(), "la reentrega es duplicado");
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert_eq!(cases.len(), 1, "el caso sigue siendo UNO");
    assert_eq!(decisions.len(), 2, "las decisiones siguen siendo DOS");
}

/// AC2: una CVE irrelevante no crea casos ni decisiones; una cuenta que
/// no es la del tenant no toca nada; y la caducidad real de una
/// excepcion produce la reevaluacion esperada.
#[tokio::test]
async fn an_irrelevant_cve_and_a_foreign_account_create_nothing_and_the_expiry_reevaluates() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let world = World::build(tmp.path()).await;
    let now = Utc::now();
    let runtime = world.the_runtime(None).await;

    // La CVE que no toca nada: el radio de impacto esta vacio, y el plan
    // no dice ni un intent.
    let irrelevant = Event::new(
        "evt-none-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": ADVISORY_IRRELEVANT, "account": ACCOUNT}),
        now,
    );
    let report = runtime
        .dispatch_planned(irrelevant, now)
        .await
        .expect("el despacho corre");
    assert!(
        report.applied.is_empty(),
        "una CVE irrelevante no aplica efecto alguno: {:?}",
        report.applied
    );
    let cases = world
        .cases
        .of_trigger(ADVISORY_IRRELEVANT)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert!(cases.is_empty(), "no hay caso para la CVE irrelevante");
    let states = world.intents.states().await.expect("los estados se leen");
    assert!(
        states.is_empty(),
        "el plan vacio no dijo intents: {states:?}"
    );

    // La cuenta ajena: la lectura del radio la cierra la puerta, y no se
    // escribe nada. Una reclamacion no es una identidad.
    let foreign = Event::new(
        "evt-foreign-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": ADVISORY, "account": "mallory"}),
        now,
    );
    let report = runtime
        .dispatch_planned(foreign, now)
        .await
        .expect("el despacho corre; el fallo es de la lectura");
    let execution = report
        .execution_of("vulnerability_impact")
        .expect("la ejecucion esta");
    assert!(
        matches!(
            execution.outcome,
            hodei_reactive::execution::ExecutionOutcome::Failed {
                retryable: true,
                ..
            }
        ),
        "la lectura que falla es un fallo con reintento: {:?}",
        execution.outcome
    );
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert!(cases.is_empty(), "una cuenta ajena no abre caso alguno");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert!(decisions.is_empty(), "una cuenta ajena no decide nada");

    // La caducidad real de una excepcion: la reevaluacion se produce.
    let expired = Event::new(
        "evt-expired-1",
        EventKind::ExceptionExpired,
        serde_json::json!({"subject": SUBJECT}),
        now,
    );
    let report = runtime
        .dispatch_planned(expired, now)
        .await
        .expect("el despacho de la caducidad corre");
    let execution = report
        .execution_of("exception_expired")
        .expect("la ejecucion del plan de caducidad esta");
    assert_eq!(
        execution.outcome,
        hodei_reactive::execution::ExecutionOutcome::Completed,
        "el plan de caducidad se completo"
    );
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert_eq!(decisions.len(), 1, "la caducidad produjo SU reevaluacion");
}

/// AC3 (incertidumbre): el recibo desconocido de la decision NO se
/// reejecuta solo; la reconciliacion explicita lo devuelve al mercado, y
/// entonces la decision se toma UNA vez.
#[tokio::test]
async fn the_unknown_decision_waits_for_the_reconcile() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let world = World::build(tmp.path()).await;
    let now = Utc::now();

    let executor: Arc<dyn IntentExecutor> = Arc::new(UnknownDecisionOnce {
        inner: Arc::new(hodei_reactive::reactions::DomainIntentExecutor::new(
            ACCOUNT,
            world.cases.clone() as Arc<dyn hodei_reactive::reactions::SecurityCaseSink>,
            world.the_requester() as Arc<dyn DecisionRequester>,
        )),
        seen: Mutex::new(HashSet::new()),
    });
    let runtime = world.the_runtime(Some(executor)).await;

    let report = runtime
        .dispatch_planned(the_cve_event("evt-unknown-1"), now)
        .await
        .expect("el despacho corre");

    // El caso se abrio (su intent se aplico) y la decision NO se tomo:
    // su recibo dice que nadie sabe donde fue a parar.
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert_eq!(cases.len(), 1, "el caso se abrio");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert!(decisions.is_empty(), "la decision incierta no se tomo");

    let unknown = report
        .executions
        .iter()
        .find(|execution| {
            matches!(
                execution.outcome,
                hodei_reactive::execution::ExecutionOutcome::Unknown { .. }
            )
        })
        .and_then(|execution| {
            execution
                .trace
                .iter()
                .find_map(|entry| entry.what.strip_prefix("intent:"))
                .map(|identity| identity.to_string())
        })
        .expect("el intent de la decision queda como desconocido");
    let states = world.intents.states().await.expect("los estados se leen");
    let unknown_state = states
        .iter()
        .find(|(identity, _state)| *identity == unknown)
        .map(|(_identity, state)| state.clone())
        .expect("el intent esta en el outbox");
    assert_eq!(unknown_state, "unknown", "el recibo dice desconocido");

    // El resume NO lo reejecuta: reejecutar sobre lo incierto es
    // repetir el efecto.
    let _ = runtime
        .resume(now + chrono::Duration::seconds(1))
        .await
        .expect("el resume corre");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert!(
        decisions.is_empty(),
        "lo desconocido no se reejecuta solo: {:?}",
        decisions.len()
    );

    // La reconciliacion explicita: alguien miro el mundo real, y AHORA
    // el intent vuelve al mercado y la decision se toma.
    world
        .intents
        .reconcile(&unknown)
        .await
        .expect("la reconciliacion entra");
    let resumed = runtime
        .resume(now + chrono::Duration::seconds(2))
        .await
        .expect("el resume tras reconciliar corre");
    assert_eq!(
        resumed.applied.len(),
        1,
        "reconciliado, la decision se aplico"
    );
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert_eq!(decisions.len(), 1, "la decision se tomo UNA vez");
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert_eq!(cases.len(), 1, "y el caso sigue siendo UNO");
}

/// AC3 (caida y reanudacion): con el ejecutor caido, los intents quedan
/// pendientes en el outbox; el resume con el ejecutor sano los aplica
/// una vez cada uno; y la reentrega del evento no crea nada de nuevo.
#[tokio::test]
async fn a_crash_is_resumed_without_duplicates() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let world = World::build(tmp.path()).await;
    let now = Utc::now();

    // El proceso "cae" en cuanto toca: los intents quedan dichos y no
    // hechos.
    let runtime = world.the_runtime(Some(Arc::new(DownExecutor))).await;
    let report = runtime
        .dispatch_planned(the_cve_event("evt-crash-1"), now)
        .await
        .expect("el despacho corre");
    assert!(
        report.applied.is_empty(),
        "con el ejecutor caido nada se aplico"
    );
    let states = world.intents.states().await.expect("los estados se leen");
    assert!(
        states.iter().all(|(_identity, state)| state == "pending"),
        "los intents quedaron pendientes: {states:?}"
    );

    // El proceso vuelve, con el ejecutor sano: la reanudacion aplica lo
    // que la caida dejo dicho.
    let healthy = world.the_runtime(None).await;
    let resumed = healthy
        .resume(now + chrono::Duration::seconds(1))
        .await
        .expect("la reanudacion corre");
    assert_eq!(
        resumed.applied.len(),
        2,
        "el caso y la decision se aplicaron"
    );

    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert_eq!(cases.len(), 1, "un caso");
    assert_eq!(decisions.len(), 1, "una decision");

    // La reanudacion de nuevo: no hay nada pendiente que reaplicar.
    let again = healthy
        .resume(now + chrono::Duration::seconds(2))
        .await
        .expect("la segunda reanudacion corre");
    assert!(again.applied.is_empty(), "lo aplicado no se reaplica");

    // Y la reentrega del evento original es duplicado, sin tocar nada.
    let delivered = healthy
        .dispatch_planned(the_cve_event("evt-crash-1"), now)
        .await
        .expect("la reentrega corre");
    assert!(
        !delivered.duplicates.is_empty(),
        "la reentrega es duplicado"
    );
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    let decisions = world
        .store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    assert_eq!(cases.len(), 1, "el caso sigue siendo UNO");
    assert_eq!(decisions.len(), 1, "la decision sigue siendo UNA");
}
