//! The flow of the intents through the outbox, with spies.
//!
//! The outbox is the seam between saying and doing, and this file walks
//! it end to end: the intent that is said crosses to claimed, the
//! receipt says what became of it, and the resume picks up whatever a
//! crash left behind -- without ever doing the same effect twice. The
//! unknown receipt does NOT come back by itself: lo incierto se
//! reconcilia, y solo entonces vuelve al mercado.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use chrono::{Duration, Utc};
use hodei_reactive::behavior::GuardOutcome;
use hodei_reactive::events::{Event, EventKind, Subscription};
use hodei_reactive::execution::Budget;
use hodei_reactive::intent::{
    InMemoryIntentOutbox, Intent, IntentExecutor, IntentOrigin, IntentOutbox, Plan,
    PlanningBehavior, ReadScope, Receipt,
};
use hodei_reactive::reactions::{
    DecisionRequester, DomainIntentExecutor, ImpactedSubject, Reevaluation, SecurityCaseSink,
};
use hodei_reactive::runtime::{InMemoryExecutionStore, ReactiveRuntime};

/// El sujeto de los intents de prueba.
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app";

/// El comportamiento que planifica UN intent de reevaluacion: leer y
/// decir, y nada mas.
struct OneIntentPlanning;

#[async_trait::async_trait]
impl PlanningBehavior for OneIntentPlanning {
    fn name(&self) -> &str {
        "one_intent"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted)
    }

    fn guard(&self, _event: &Event) -> GuardOutcome {
        GuardOutcome::Pass
    }

    async fn plan(
        &self,
        event: &Event,
        scope: &ReadScope,
    ) -> Result<Plan, hodei_reactive::behavior::BehaviorError> {
        let intent = Intent::new(
            IntentOrigin::of(self.name(), event, &scope.origin.causation),
            scope.tenant.clone(),
            "decision_reevaluation",
            format!("decision:{}", scope.origin.event_id),
            scope.origin.event_id.clone(),
            serde_json::json!({"subject": SUBJECT, "reason": "vulnerability_impacted", "at": scope.at.to_rfc3339()}),
        );
        Ok(Plan {
            intents: vec![intent],
            events: Vec::new(),
        })
    }
}

/// The event that starts everything.
fn the_event() -> Event {
    Event::new(
        "evt-flow-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-1", "account": "acme"}),
        Utc::now(),
    )
}

/// What the spy executor answers, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answer {
    /// The first apply lands; the rest land too.
    Applied,
    /// The first apply fails with a retryable receipt; the rest land.
    FailsOnce,
    /// The first apply answers unknown; after the reconciliation, the
    /// world answers and the retry lands.
    UnknownOnce,
}

/// The spy executor: it counts the touches and answers what its mode
/// says, in the words of the receipts.
struct SpyExecutor {
    calls: AtomicUsize,
    answer: Answer,
}

impl SpyExecutor {
    fn with(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            answer,
        })
    }

    fn touches(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl IntentExecutor for SpyExecutor {
    async fn apply(&self, _intent: &Intent) -> Receipt {
        let nth = self.calls.fetch_add(1, Ordering::SeqCst);
        match self.answer {
            Answer::Applied => Receipt::Applied,
            Answer::FailsOnce => {
                if nth == 0 {
                    Receipt::Failed {
                        reason: "the sink is down".to_string(),
                        retryable: true,
                    }
                } else {
                    Receipt::Applied
                }
            }
            Answer::UnknownOnce => {
                if nth == 0 {
                    Receipt::Unknown {
                        reason: "the answer never came".to_string(),
                    }
                } else {
                    Receipt::Applied
                }
            }
        }
    }
}

/// The runtime with one planner, one spy executor, the outbox in memory
/// and the execution store in memory too.
fn the_runtime(answer: Answer) -> (ReactiveRuntime, Arc<SpyExecutor>, Arc<InMemoryIntentOutbox>) {
    let outbox = Arc::new(InMemoryIntentOutbox::empty());
    let executor = SpyExecutor::with(answer);
    let mut runtime =
        ReactiveRuntime::new(Arc::new(InMemoryExecutionStore::empty()), Budget::default())
            .with_tenant("acme");
    runtime.with_outbox(outbox.clone());
    runtime.register_planned(Arc::new(OneIntentPlanning), executor.clone());
    (runtime, executor, outbox)
}

/// La identidad del intent que el planner dice.
fn the_identity() -> String {
    "intent:one_intent:evt-flow-1:decision:evt-flow-1".to_string()
}

/// El intent cruza el outbox de punta a punta: pendiente, reclamado,
/// aplicado; y la misma entrega dos veces no lo toca dos veces.
#[tokio::test]
async fn the_intent_crosses_the_outbox_and_lands_once() {
    let (runtime, executor, outbox) = the_runtime(Answer::Applied);

    let first = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el despacho corre");
    assert_eq!(first.applied.len(), 1, "el efecto se aplico");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("applied".to_string()),
        "el intent quedo aplicado en el outbox"
    );

    // La reentrega del mismo evento: duplicado, sin toques nuevos.
    let second = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("la reentrega corre");
    assert!(!second.duplicates.is_empty(), "la reentrega es duplicado");
    assert!(
        second.applied.is_empty(),
        "el efecto ya aplicado no se repite"
    );
    assert_eq!(executor.touches(), 1, "un intent, un toque, ni uno mas");
}

/// La caida entre decir y hacer: el recibo retryable devuelve el intent
/// al mercado, y el resume lo reclama y lo aplica, sin repetir el
/// efecto.
#[tokio::test]
async fn a_crash_between_saying_and_doing_is_recovered_by_the_resume() {
    let (runtime, executor, outbox) = the_runtime(Answer::FailsOnce);
    let now = Utc::now();

    let report = runtime
        .dispatch_planned(the_event(), now)
        .await
        .expect("el despacho corre");
    assert!(report.applied.is_empty(), "el primer toque fallo");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("pending".to_string()),
        "el fallo que se puede reintentar vuelve al mercado"
    );

    // La reanudacion: el intent pendiente se reclama y se aplica.
    let resumed = runtime
        .resume(now + Duration::seconds(1))
        .await
        .expect("la reanudacion corre");
    assert_eq!(resumed.applied.len(), 1, "el intent recuperado se aplico");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("applied".to_string()),
        "el recibo de la reanudacion dice aplicado"
    );
    assert_eq!(
        executor.touches(),
        2,
        "dos toques: el que fallo y el que no"
    );
    assert_eq!(
        runtime.store().applied_effects().await.len(),
        1,
        "y el efecto es UNO: la clave de idempotencia manda"
    );
}

/// Lo desconocido NO vuelve por si solo: el resume no lo reejecuta, la
/// reconciliacion explicita lo devuelve al mercado, y entonces si.
#[tokio::test]
async fn the_unknown_waits_for_the_reconcile() {
    let (runtime, executor, outbox) = the_runtime(Answer::UnknownOnce);
    let now = Utc::now();

    runtime
        .dispatch_planned(the_event(), now)
        .await
        .expect("el despacho corre");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("unknown".to_string()),
        "el recibo dice que nadie sabe"
    );

    // El resume NO toca lo desconocido: reejecutar sobre un resultado
    // incierto es repetir el efecto, y la clave de idempotencia lo
    // prohibia.
    let _ = runtime
        .resume(now + Duration::seconds(1))
        .await
        .expect("el resume corre");
    assert_eq!(executor.touches(), 1, "lo desconocido no se reejecuta solo");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("unknown".to_string()),
        "sigue desconocido"
    );

    // La reconciliacion: alguien miro el mundo real, y AHORA vuelve.
    outbox
        .reconcile(&the_identity())
        .await
        .expect("la reconciliacion entra");
    let resumed = runtime
        .resume(now + Duration::seconds(2))
        .await
        .expect("el resume tras reconciliar corre");
    assert_eq!(resumed.applied.len(), 1, "reconciliado, se aplica");
    assert_eq!(
        outbox.state_of(&the_identity()),
        Some("applied".to_string()),
        "el recibo final dice aplicado"
    );
    assert_eq!(
        runtime.store().applied_effects().await.len(),
        1,
        "el efecto, una vez"
    );
}

/// El ejecutor que no es del tenant del intent se niega a tocar: una
/// reclamacion no es una identidad.
#[tokio::test]
async fn an_executor_of_another_tenant_refuses_to_touch() {
    let cases = Arc::new(SpyCases::default());
    let decisions = Arc::new(SpyDecisions::default());
    let executor = DomainIntentExecutor::new(
        "acme",
        cases.clone() as Arc<dyn SecurityCaseSink>,
        decisions.clone() as Arc<dyn DecisionRequester>,
    );

    let event = the_event();
    let intent = Intent::new(
        IntentOrigin::of("one_intent", &event, &[event.id.clone()]),
        "mallory",
        "decision_reevaluation",
        "decision:mallory",
        "evt-1",
        serde_json::json!({"subject": SUBJECT, "reason": "vulnerability_impacted"}),
    );

    let receipt = executor.apply(&intent).await;
    assert!(
        matches!(
            receipt,
            Receipt::Failed {
                retryable: false,
                ..
            }
        ),
        "un intent de otro tenant se rechaza sin reintento: {receipt:?}"
    );
    assert_eq!(
        decisions.requests.load(Ordering::SeqCst),
        0,
        "no se pidio decision alguna"
    );
    assert_eq!(
        cases.opens.load(Ordering::SeqCst),
        0,
        "no se abrio caso alguno"
    );
}

/// El caso de seguridad que el ejecutor toca, con los datos del intent:
/// advisory, sujetos impactados y el momento de la observacion.
#[tokio::test]
async fn the_case_intent_opens_the_case_of_its_tenant() {
    let cases = Arc::new(SpyCases::default());
    let decisions = Arc::new(SpyDecisions::default());
    let executor = DomainIntentExecutor::new(
        "acme",
        cases.clone() as Arc<dyn SecurityCaseSink>,
        decisions.clone() as Arc<dyn DecisionRequester>,
    );

    let event = the_event();
    let intent = Intent::new(
        IntentOrigin::of("vulnerability_impact", &event, &[event.id.clone()]),
        "acme",
        "security_case",
        "case:GHSA-1",
        "evt-flow-1",
        serde_json::json!({
            "advisory": "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-1",
            "at": Utc::now().to_rfc3339(),
            "impacted": [{"subject": SUBJECT, "deployment": null, "depth": 1}],
        }),
    );

    let receipt = executor.apply(&intent).await;
    assert_eq!(receipt, Receipt::Applied, "el caso se abrio: {receipt:?}");
    assert_eq!(cases.opens.load(Ordering::SeqCst), 1);
    assert_eq!(decisions.requests.load(Ordering::SeqCst), 0);
}

/// El sumidero de casos que cuenta lo que toca.
#[derive(Default)]
struct SpyCases {
    opens: AtomicUsize,
}

#[async_trait::async_trait]
impl SecurityCaseSink for SpyCases {
    async fn open_or_update(
        &self,
        _advisory: &str,
        _impacted: &[ImpactedSubject],
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<String, String> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok("case-1".to_string())
    }
}

/// El pidiente de decisiones que cuenta lo que toca.
#[derive(Default)]
struct SpyDecisions {
    requests: AtomicUsize,
}

#[async_trait::async_trait]
impl DecisionRequester for SpyDecisions {
    async fn request_reevaluation(&self, _request: Reevaluation) -> Result<String, String> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok("dec-1".to_string())
    }
}
