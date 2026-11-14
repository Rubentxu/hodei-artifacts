//! El crash real: el proceso muere, la base queda, y el proceso vuelve.
//!
//! Los tres ventanas de fallo son las del outbox: morir ANTES de
//! reservar el efecto, morir DESPUES de reclamarlo, y morir DESPUES de
//! que el efecto externo ocurrio y ANTES del acuse. Cada ventana se
//! prueba con un proceso hijo DE VERDAD: el padre lo invoca, el hijo
//! despacha con el failpoint y el failpoint lo mata (SIGABRT), y el
//! padre reabre la base y recupera en el arranque. Lo confirmado no se
//! repite; lo pendiente vuelve; y lo que nadie sabe espera su
//! reconciliacion.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_reactive::events::{Event, EventKind};
use hodei_reactive::execution::Budget;
use hodei_reactive::intent::{Intent, IntentExecutor, IntentOutbox, Receipt};
use hodei_reactive::reactions::{
    DecisionRequester, DomainIntentExecutor, PlannedExceptionExpiryReaction,
    PlannedVulnerabilityReaction, SecurityCaseSink,
};
use hodei_reactive::runtime::{ExecutionStore, ReactiveRuntime};
use hodei_risk_storage::reactions::{SurrealBlastRadius, SurrealSecurityCaseStore};
use hodei_trust_policy::ports::DecisionStore;
use hodei_trust_policy::promotion::PromotionTarget;
use hodei_trust_policy::service::PromotionService;
use hodei_trust_policy::{PolicyRegistry, Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::intents::{IntentDiagnostic, SurrealIntentStore};
use hodei_trust_policy_storage::reactions::{EmptyContextSource, PromotionDecisionRequester};
use hodei_trust_policy_storage::{StaticPromotionTargets, SurrealDecisionStore};
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// La cuenta del crash.
const ACCOUNT: &str = "acme";

/// El sujeto del crash.
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0";

/// La vulnerabilidad del crash.
const ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-crash-0001";

/// La politica de produccion, la misma de siempre.
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

/// El evento de la CVE que el hijo lleva cuando muere.
fn the_cve_event() -> Event {
    Event::new(
        "evt-crash",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": ADVISORY, "account": ACCOUNT}),
        Utc::now(),
    )
}

/// El mundo sobre una base: la abren el hijo, el padre que siembra y el
/// padre que recupera, UNO POR VEZ, porque RocksDb se abre en exclusiva.
struct World {
    db: Arc<Surreal<Db>>,
    graph: Arc<hodei_evidence_storage::surreal::graph::SurrealGraphStore<Db>>,
    cases: Arc<SurrealSecurityCaseStore<Db>>,
    radius: Arc<SurrealBlastRadius<Db>>,
    intents: Arc<SurrealIntentStore<Db>>,
    executor: Arc<DomainIntentExecutor>,
}

impl World {
    /// Abre la base, monta los adaptadores y activa SU politica en
    /// memoria: la politica no es durable, y cada proceso trae la suya.
    async fn open(path: &std::path::Path) -> Self {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
                .await
                .expect("la base de datos abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el espacio de nombres");

        let graph =
            Arc::new(hodei_evidence_storage::surreal::graph::SurrealGraphStore::new(db.clone()));
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
        let service = Arc::new(PromotionService::new(
            store.clone(),
            Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
                distribution: "production".to_string(),
                account: ACCOUNT.to_string(),
            }])),
            policies,
        ));
        let requester = Arc::new(PromotionDecisionRequester::new(
            service,
            store,
            Arc::new(EmptyContextSource),
            "production",
            "production",
            "system",
        ));
        let executor = Arc::new(DomainIntentExecutor::new(
            ACCOUNT,
            cases.clone() as Arc<dyn SecurityCaseSink>,
            requester as Arc<dyn DecisionRequester>,
        ));

        Self {
            db,
            graph,
            cases,
            radius,
            intents,
            executor,
        }
    }

    /// Siembra la capa de riesgo: el artefacto, su componente vulnerable,
    /// la vulnerabilidad y el despliegue. Solo la hace el primer proceso.
    async fn seed_risk_layer(&self) {
        use hodei_evidence::domain::graph::{
            GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
        };
        use hodei_evidence::ports::graph::GraphWriter;
        use hodei_evidence::EvidenceId;

        let evidence = EvidenceId::new("ev-crash-1");
        let now = Utc::now();
        let component = Hrn::from_string(&format!(
            "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-vuln"
        ))
        .expect("un HRN");
        let deployment = Hrn::from_string(&format!(
            "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Deployment/production"
        ))
        .expect("un HRN");

        let changes = GraphChangeSet::empty()
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

    /// El runtime planificado, con el ejecutor y el outbox que se le
    /// den: el hijo pasa los failpoints, el padre los reales.
    fn the_runtime(
        &self,
        executor: Option<Arc<dyn IntentExecutor>>,
        outbox: Option<Arc<dyn IntentOutbox>>,
    ) -> ReactiveRuntime {
        let store: Arc<dyn ExecutionStore> = self.intents.clone();
        let outbox = outbox.unwrap_or_else(|| self.intents.clone() as Arc<dyn IntentOutbox>);
        let mut runtime = ReactiveRuntime::new(store, Budget::default()).with_tenant(ACCOUNT);
        runtime.with_outbox(outbox);
        let executor = executor.unwrap_or_else(|| self.executor.clone() as Arc<dyn IntentExecutor>);
        runtime.register_planned(
            Arc::new(PlannedVulnerabilityReaction::new(self.radius.clone())),
            executor.clone(),
        );
        runtime.register_planned(Arc::new(PlannedExceptionExpiryReaction::new()), executor);
        runtime
    }
}

/// La ventana en la que el hijo muere.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Window {
    /// Muere ANTES de reservar: el intent queda dicho y pendiente.
    BeforeClaim,
    /// Muere DESPUES de reclamar: el intent queda reclamado, con su
    /// lease, y nadie lo trabajo.
    AfterClaim,
}

/// El outbox con el punto de fallo: delega TODO, y en SU ventana mata el
/// proceso.
struct AbortOutbox {
    inner: Arc<SurrealIntentStore<Db>>,
    window: Window,
}

#[async_trait::async_trait]
impl IntentOutbox for AbortOutbox {
    async fn put_pending(&self, intent: &Intent) -> Result<(), String> {
        self.inner.put_pending(intent).await
    }

    async fn claim(
        &self,
        identity: &str,
        lease_owner: &str,
        lease_until: chrono::DateTime<Utc>,
        now: chrono::DateTime<Utc>,
    ) -> Result<Option<Intent>, String> {
        if self.window == Window::BeforeClaim {
            std::process::abort();
        }
        let claimed = self
            .inner
            .claim(identity, lease_owner, lease_until, now)
            .await;
        if self.window == Window::AfterClaim {
            std::process::abort();
        }
        claimed
    }

    async fn record_receipt(&self, identity: &str, receipt: &Receipt) -> Result<(), String> {
        self.inner.record_receipt(identity, receipt).await
    }

    async fn recover_pending(&self, now: chrono::DateTime<Utc>) -> Result<Vec<Intent>, String> {
        self.inner.recover_pending(now).await
    }

    async fn reconcile(&self, identity: &str) -> Result<(), String> {
        self.inner.reconcile(identity).await
    }
}

/// El ejecutor que deja el efecto AFUERA y muere antes del acuse: el
/// mundo externo ya cambio, y el outbox no lo sabe.
struct AbortAfterEffect {
    inner: Arc<DomainIntentExecutor>,
}

#[async_trait::async_trait]
impl IntentExecutor for AbortAfterEffect {
    async fn apply(&self, intent: &Intent) -> Receipt {
        let _receipt = self.inner.apply(intent).await;
        std::process::abort();
    }
}

/// El hijo: SOLO corre cuando el padre lo invoca, y muere en la ventana
/// que el padre le pide. Sin el env, es un test que no hace nada.
#[tokio::test]
async fn the_child_dies_at_its_window() {
    let Ok(window) = std::env::var("HODEI_CRASH_WINDOW") else {
        return;
    };
    if std::env::var("HODEI_CRASH_CHILD").as_deref().unwrap_or("") != "1" {
        return;
    }
    // La durabilidad que un crash real pide: los commits fsyncan el
    // WAL, porque el default del backend (SURREAL_SYNC_DATA=false)
    // pierde lo escrito si el proceso muere sin cerrar. Es un requisito
    // operativo de Artifacts, y el hijo lo lleva en su arranque, antes
    // de abrir la base.
    std::env::set_var("SURREAL_SYNC_DATA", "true");
    let db_path = std::env::var("HODEI_CRASH_DB").expect("la ruta de la base");
    let world = World::open(std::path::Path::new(&db_path)).await;
    let now = Utc::now();

    match window.as_str() {
        "before-claim" | "after-claim" => {
            let w = if window == "before-claim" {
                Window::BeforeClaim
            } else {
                Window::AfterClaim
            };
            let outbox: Arc<dyn IntentOutbox> = Arc::new(AbortOutbox {
                inner: world.intents.clone(),
                window: w,
            });
            let runtime = world.the_runtime(None, Some(outbox));
            let _ = runtime.dispatch_planned(the_cve_event(), now).await;
        }
        "after-effect" => {
            let executor: Arc<dyn IntentExecutor> = Arc::new(AbortAfterEffect {
                inner: world.executor.clone(),
            });
            let runtime = world.the_runtime(Some(executor), None);
            let _ = runtime.dispatch_planned(the_cve_event(), now).await;
        }
        "graceful" => {
            // El control: despacha y sale LIMPIO, sin failpoint.
            let runtime = world.the_runtime(None, None);
            let _ = runtime.dispatch_planned(the_cve_event(), now).await;
            return;
        }
        other => panic!("ventana desconocida: {other}"),
    }
    // No se llega aqui: el failpoint mato el proceso.
    std::process::abort();
}

/// Invoca al hijo en su ventana, espera su muerte y reabre la base: el
/// proceso vuelve.
async fn kill_and_restart_at(window: &str) -> (std::path::PathBuf, World) {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");

    // El primer proceso: siembra el mundo y muere de viejo, dejando la
    // base libre para el hijo.
    {
        let world = World::open(tmp.path()).await;
        world.seed_risk_layer().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // El hijo: despacha y muere en SU ventana.
    let mut child = std::process::Command::new(std::env::current_exe().expect("el binario"))
        .arg("the_child_dies_at_its_window")
        .arg("--exact")
        .arg("--nocapture")
        .env("HODEI_CRASH_CHILD", "1")
        .env("HODEI_CRASH_WINDOW", window)
        .env("HODEI_CRASH_DB", tmp.path().to_string_lossy().to_string())
        .spawn()
        .expect("el hijo arranca");
    let status = child.wait().expect("el hijo acaba");
    if window != "graceful" {
        let by_signal = std::os::unix::process::ExitStatusExt::signal(&status);
        assert_eq!(
            by_signal,
            Some(6),
            "el hijo murio por SIGABRT del failpoint, no de otra manera: {status:?}"
        );
    } else {
        assert!(status.success(), "el control salio limpio: {status:?}");
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    // El proceso vuelve: otra World sobre la MISMA base.
    let root = tmp.path().to_path_buf();
    (root, World::open(tmp.path()).await)
}

/// Las cuentas de la ruta, despues de la recuperacion.
async fn the_counts(world: &World) -> (usize, usize, Vec<IntentDiagnostic>) {
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    let decisions = world
        .the_decisions()
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .map_err(|error| error.to_string())
        .expect("las decisiones se leen");
    let diagnostics = world
        .intents
        .diagnose(ACCOUNT)
        .await
        .expect("el diagnostico");
    (cases.len(), decisions.len(), diagnostics)
}

impl World {
    /// El store de decisiones, para leerlas.
    fn the_decisions(&self) -> Arc<SurrealDecisionStore<Db>> {
        Arc::new(SurrealDecisionStore::new(self.db.clone()))
    }
}

/// Ventana ANTES de reservar: el intent quedo dicho y pendiente; el
/// arranque lo recupera y la ruta entera se aplica una vez.
#[tokio::test]
async fn a_kill_before_the_claim_leaves_the_route_ready_to_resume() {
    let (_tmp, world) = kill_and_restart_at("before-claim").await;

    // La recuperacion en el arranque, con un momento despues de
    // cualquier lease que el hijo pudiera tomar.
    let now = Utc::now() + Duration::seconds(120);
    let report = world
        .the_runtime(None, None)
        .resume(now)
        .await
        .expect("la recuperacion en el arranque corre");
    assert_eq!(
        report.applied.len(),
        2,
        "el caso y la decision se aplicaron"
    );

    let (cases, decisions, diagnostics) = the_counts(&world).await;
    assert_eq!(cases, 1, "un caso");
    assert_eq!(decisions, 1, "una decision");
    assert_eq!(diagnostics.len(), 2, "dos intents en el outbox");
    assert!(
        diagnostics
            .iter()
            .all(|row| row.state == "applied" && row.failure_reason.is_none()),
        "todo aplicado y sin motivo de fallo: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|row| row.tenant == ACCOUNT),
        "el diagnostico nombra al tenant de cada intent"
    );
    assert!(
        diagnostics
            .iter()
            .all(|row| row.cause.contains(&"evt-crash".to_string())),
        "la causa de cada intent dice el evento que la trajo"
    );
}

/// Ventana DESPUES de reclamar: el intent murio reclamado, con su lease
/// vivo; el arranque con un momento DESPUES del lease lo devuelve al
/// mercado y la ruta se aplica una vez. Dos procesos tocaron el mismo
/// intent, y no hay doble efecto.
#[tokio::test]
async fn a_kill_after_the_claim_is_recovered_when_the_lease_dies() {
    let (_tmp, world) = kill_and_restart_at("after-claim").await;

    // ANTES del lease: lo PENDIENTE se aplica (las decisiones de la
    // ruta dijeron y no trabajaron), pero el intent que el hijo murio
    // RECLAMANDO sigue bajo su lease y NO se toca.
    let too_early = world
        .the_runtime(None, None)
        .resume(Utc::now())
        .await
        .expect("el primer resume corre");
    assert_eq!(
        too_early.applied.len(),
        1,
        "solo lo pendiente se aplica: {:?}",
        too_early.applied
    );
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert!(
        cases.is_empty(),
        "el intent reclamado con lease vivo no se toca antes de tiempo"
    );

    // DESPUES del lease: el intent reclamado vuelve al mercado y se
    // aplica; la decision ya estaba aplicada de la primera reanudacion,
    // y no se repite.
    let now = Utc::now() + Duration::seconds(120);
    let report = world
        .the_runtime(None, None)
        .resume(now)
        .await
        .expect("la recuperacion corre");
    assert_eq!(
        report.applied.len(),
        1,
        "solo el caso reclamado se aplica: {:?}",
        report.applied
    );

    let (cases, decisions, diagnostics) = the_counts(&world).await;
    assert_eq!(cases, 1, "un caso");
    assert_eq!(decisions, 1, "una decision");
    assert!(
        diagnostics.iter().all(|row| row.state == "applied"),
        "todo aplicado: {diagnostics:?}"
    );
}

/// Ventana DESPUES del efecto externo y ANTES del acuse: el caso YA
/// existe en el mundo y el outbox no lo sabe. La recuperacion vuelve a
/// aplicar, y el sumidero idempotente deja UN caso y UNA decision: el
/// efecto confirmado no se repite.
#[tokio::test]
async fn a_kill_after_the_effect_and_before_the_receipt_does_not_repeat_it() {
    let (_tmp, world) = kill_and_restart_at("after-effect").await;

    // El efecto AFUERA ya ocurrio: el caso existe ANTES de la
    // recuperacion.
    let cases = world
        .cases
        .of_trigger(ADVISORY)
        .await
        .map_err(|error| error.to_string())
        .expect("los casos se leen");
    assert_eq!(cases.len(), 1, "el caso del hijo muerto esta en el mundo");

    // La recuperacion: el intent reclamado vuelve al mercado y se
    // re-aplica; el sumidero idempotente NO abre otro caso.
    let now = Utc::now() + Duration::seconds(120);
    let report = world
        .the_runtime(None, None)
        .resume(now)
        .await
        .expect("la recuperacion corre");
    assert_eq!(
        report.applied.len(),
        2,
        "el caso y la decision se aplicaron"
    );

    let (cases, decisions, diagnostics) = the_counts(&world).await;
    assert_eq!(cases, 1, "UN caso, aunque el executor toco dos veces");
    assert_eq!(decisions, 1, "UNA decision");
    assert!(
        diagnostics.iter().all(|row| row.state == "applied"),
        "todo aplicado: {diagnostics:?}"
    );
}

/// Lo que queda INCERTO sobrevive al reinicio marcado, NO se reejecuta
/// solo, y la reconciliacion explicita lo devuelve al mercado.
#[tokio::test]
async fn an_unknown_result_survives_the_restart_marked_and_reconciliable() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");

    // El primer proceso: siembra, despacha con un ejecutor que responde
    // desconocido, y muere de viejo.
    {
        let world = World::open(tmp.path()).await;
        world.seed_risk_layer().await;
        let executor: Arc<dyn IntentExecutor> = Arc::new(UnknownDecisionExecutor {
            inner: world.executor.clone(),
        });
        let runtime = world.the_runtime(Some(executor), None);
        let _ = runtime
            .dispatch_planned(the_cve_event(), Utc::now())
            .await
            .expect("el despacho corre");
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // El proceso vuelve: lo desconocido esta MARCADO, con su motivo.
    let world = World::open(tmp.path()).await;
    let diagnostics = world
        .intents
        .diagnose(ACCOUNT)
        .await
        .expect("el diagnostico");
    let unknown: Vec<&IntentDiagnostic> = diagnostics
        .iter()
        .filter(|row| row.state == "unknown")
        .collect();
    assert_eq!(
        unknown.len(),
        1,
        "la decision quedo desconocida: {diagnostics:?}"
    );
    assert!(
        unknown[0].failure_reason.is_some(),
        "el motivo de lo desconocido esta en la vista"
    );

    // El arranque NO lo reejecuta.
    let report = world
        .the_runtime(None, None)
        .resume(Utc::now() + Duration::seconds(120))
        .await
        .expect("el arranque corre");
    assert!(
        report.applied.is_empty(),
        "lo desconocido no se reejecuta solo: {:?}",
        report.applied
    );

    // La reconciliacion explicita, y despues si.
    let identity = diagnostics
        .iter()
        .find(|row| row.state == "unknown")
        .map(|row| format!("intent:{}:{}:{}", row.run, row.event_id, row.effect))
        .expect("la identidad del desconocido");
    world
        .intents
        .reconcile(&identity)
        .await
        .expect("la reconciliacion entra");
    let report = world
        .the_runtime(None, None)
        .resume(Utc::now() + Duration::seconds(120))
        .await
        .expect("el resume tras reconciliar corre");
    assert_eq!(
        report.applied.len(),
        1,
        "reconciliado, la decision se aplica"
    );

    let (cases, decisions, diagnostics) = the_counts(&world).await;
    assert_eq!(cases, 1, "un caso");
    assert_eq!(decisions, 1, "una decision");
    assert!(
        diagnostics.iter().all(|row| row.state == "applied"),
        "todo aplicado al final: {diagnostics:?}"
    );
}

/// El ejecutor que responde desconocido la primera vez que toca una
/// decision, y delega despues.
struct UnknownDecisionExecutor {
    inner: Arc<DomainIntentExecutor>,
}

#[async_trait::async_trait]
impl IntentExecutor for UnknownDecisionExecutor {
    async fn apply(&self, intent: &Intent) -> Receipt {
        if intent.kind == "decision_reevaluation" {
            return Receipt::Unknown {
                reason: "the answer never came".to_string(),
            };
        }
        self.inner.apply(intent).await
    }
}

/// La vista de estado: tenant-scoped, sin secretos, con lo que el
/// operador necesita para saber que pasa.
#[tokio::test]
async fn the_state_view_is_tenant_scoped_and_carries_no_secrets() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let world = World::open(tmp.path()).await;
    world.seed_risk_layer().await;

    let runtime = world.the_runtime(None, None);
    let _ = runtime
        .dispatch_planned(the_cve_event(), Utc::now())
        .await
        .expect("el despacho corre");

    // Lo que ve SU tenant: la ruta entera, aplicada.
    let diagnostics = world
        .intents
        .diagnose(ACCOUNT)
        .await
        .expect("el diagnostico");
    assert_eq!(
        diagnostics.len(),
        2,
        "la ruta dijo dos intents: {diagnostics:?}"
    );
    for row in &diagnostics {
        assert_eq!(row.tenant, ACCOUNT);
        assert_eq!(row.run, "vulnerability_impact");
        assert_eq!(row.state, "applied");
        assert!(row.failure_reason.is_none());
        assert!(!row.effect.is_empty());
        assert_eq!(row.event_id, "evt-crash");
    }
    let serialized = serde_json::to_string(&diagnostics).expect("el diagnostico se serializa");
    assert!(
        !serialized.contains("payload"),
        "la vista no lleva payload alguno: {serialized}"
    );

    // Lo que ve OTRO tenant: nada. Una vista que contesta de mas es una
    // vista que filtra.
    let foreign = world
        .intents
        .diagnose("mallory")
        .await
        .expect("el diagnostico ajeno");
    assert!(foreign.is_empty(), "otro tenant no ve nada: {foreign:?}");

    // Y la vista sin tenant no contesta.
    let outcome = world.intents.diagnose("").await;
    assert!(
        outcome.is_err(),
        "sin tenant no hay diagnostico: {outcome:?}"
    );
}

/// El control: el hijo despacha y sale LIMPIO; la base tiene TODO lo
/// escrito. Aisla la durabilidad al kill de cualquier otro problema.
#[tokio::test]
async fn the_control_child_that_exits_cleanly_leaves_everything() {
    let (_root, world) = kill_and_restart_at("graceful").await;
    let states = world.intents.states().await.expect("los estados");
    assert_eq!(states.len(), 2, "el hijo limpio dejo dos intents");
}
