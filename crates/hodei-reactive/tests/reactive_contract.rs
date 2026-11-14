//! The contract of the runtime of the reactions.
//!
//! Six things are promised and six things are checked:
//!
//! - A behaviour registers, subscribes to an event, and runs when the
//!   event matches.
//! - The execution trace is durable, and a failure retries with the
//!   declared semantics.
//! - The budget guard cuts the chain when it is exceeded, and the cut is
//!   in the trace.
//! - The side effects are idempotent, and repeating the reaction does not
//!   duplicate the effect.
//! - A new vulnerability over a deployed artefact triggers the
//!   reevaluation and, if it applies, the case.
//! - The expiry of an exception triggers its reaction.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use hodei_reactive::behavior::{Behavior, BehaviorError, Effect, GuardOutcome, Reaction};
use hodei_reactive::events::{Event, EventKind, PayloadPredicate, Subscription};
use hodei_reactive::execution::{Budget, ExecutionOutcome};
use hodei_reactive::reactions::{
    BlastRadiusSource, DecisionRequester, ExceptionExpiryReaction, ImpactedSubject, ReasonCode,
    Reevaluation, SecurityCaseSink, VulnerabilityReaction,
};
use hodei_reactive::runtime::{
    DispatchReport, ExecutionStore, InMemoryExecutionStore, ReactiveRuntime,
};

/// Un behaviour de prueba que cuenta lo que hizo.
struct CountingBehavior {
    name: String,
    subscription: Subscription,
    calls: Arc<Mutex<usize>>,
    failure: Option<BehaviorError>,
    caused: Option<Event>,
}

impl CountingBehavior {
    fn new(name: &str, subscription: Subscription) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            subscription,
            calls: Arc::new(Mutex::new(0)),
            failure: None,
            caused: None,
        })
    }

    fn failing(name: &str, subscription: Subscription, failure: BehaviorError) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            subscription,
            calls: Arc::new(Mutex::new(0)),
            failure: Some(failure),
            caused: None,
        })
    }

    fn causing(name: &str, subscription: Subscription, caused: Event) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            subscription,
            calls: Arc::new(Mutex::new(0)),
            failure: None,
            caused: Some(caused),
        })
    }

    fn calls(&self) -> usize {
        *self.calls.lock().expect("the counter is not poisoned")
    }
}

#[async_trait]
impl Behavior for CountingBehavior {
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
        *self.calls.lock().expect("the counter is not poisoned") += 1;
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let mut events = Vec::new();
        if let Some(caused) = &self.caused {
            events.push(caused.clone());
        }
        Ok(Reaction {
            events,
            effects: vec![Effect::new(
                "notification",
                format!("notify:{}", self.name),
                serde_json::json!({"behaviour": self.name}),
            )],
        })
    }
}

/// La cuenta de las decisiones pedidas.
struct RecordingDecisions {
    asked: Arc<Mutex<Vec<(String, ReasonCode)>>>,
}

impl RecordingDecisions {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            asked: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

#[async_trait]
impl DecisionRequester for RecordingDecisions {
    async fn request_reevaluation(&self, request: Reevaluation) -> Result<String, String> {
        self.asked
            .lock()
            .expect("the recorder is not poisoned")
            .push((request.subject.clone(), request.reason));
        Ok(format!("dec-{}", request.subject))
    }
}

/// El radio de impacto de prueba.
struct StubRadius {
    impacted: Vec<ImpactedSubject>,
}

#[async_trait]
impl BlastRadiusSource for StubRadius {
    async fn impacted(
        &self,
        _account: &str,
        _advisory: &str,
    ) -> Result<Vec<ImpactedSubject>, String> {
        Ok(self.impacted.clone())
    }
}

/// El caso de prueba.
#[derive(Clone)]
struct StubCases {
    opened: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl SecurityCaseSink for StubCases {
    async fn open_or_update(
        &self,
        advisory: &str,
        _impacted: &[ImpactedSubject],
        _now: chrono::DateTime<Utc>,
    ) -> Result<String, String> {
        self.opened
            .lock()
            .expect("the recorder is not poisoned")
            .push(advisory.to_string());
        Ok(format!("case-{advisory}"))
    }
}

// ───────────────────────────── AC1 ─────────────────────────────

/// Un behavior se registra, se suscribe a un evento y se ejecuta cuando el
/// evento encaja.
#[tokio::test]
async fn a_behaviour_runs_when_the_event_matches() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());

    let behaviour = CountingBehavior::new(
        "counting",
        Subscription::to(EventKind::VulnerabilityAccepted),
    );
    runtime.register(behaviour.clone());

    // Un evento que no encaja: el comportamiento no corre.
    let other = Event::new(
        "ev-other",
        EventKind::ExceptionExpired,
        serde_json::json!({}),
        Utc::now(),
    );
    runtime.dispatch(other, Utc::now()).await.expect("dispatch");
    assert_eq!(behaviour.calls(), 0, "the behaviour is not interested");

    // Y uno que si.
    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");
    assert_eq!(behaviour.calls(), 1);
    let execution = report
        .execution_of("counting")
        .expect("the execution is there");
    assert_eq!(execution.outcome, ExecutionOutcome::Completed);
    assert_eq!(execution.event_id, "ev-1");
}

// ───────────────────────────── AC2 ─────────────────────────────

/// La traza de ejecucion es durable, y un fallo reintenta con la
/// semantica declarada.
#[tokio::test]
async fn the_trace_is_durable_and_a_failure_retries() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());

    runtime.register(CountingBehavior::failing(
        "failing",
        Subscription::to(EventKind::VulnerabilityAccepted),
        BehaviorError::transient("the sink is down"),
    ));

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );

    // La primera vez falla, y el fallo queda escrito con su motivo.
    let report = runtime
        .dispatch(event.clone(), Utc::now())
        .await
        .expect("dispatch");
    let execution = report
        .execution_of("failing")
        .expect("the execution is there");
    assert!(execution.is_failure());
    assert!(
        execution.is_retryable_failure(),
        "and the failure is retryable"
    );
    assert_eq!(execution.attempts, 1);

    // Y el registro es durable: esta en el almacen, no solo en el informe.
    let stored = store
        .execution_of("failing:ev-1")
        .await
        .expect("the record is there");
    assert!(stored.is_failure());
    assert!(
        stored
            .trace
            .iter()
            .any(|entry| entry.what.contains("the sink is down")),
        "and the trace says why it failed"
    );

    // La segunda vez reintenta, y el numero de intentos sube.
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");
    let execution = report
        .execution_of("failing")
        .expect("the execution is there");
    assert_eq!(execution.attempts, 2, "the retry counts as an attempt");
}

// ───────────────────────────── AC3 ─────────────────────────────

/// La guarda de presupuesto corta la cadena cuando se excede, y el corte
/// queda en la traza.
#[tokio::test]
async fn the_budget_cuts_the_chain_and_the_cut_is_in_the_trace() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(
        store.clone(),
        Budget {
            max_executions: 1,
            max_depth: 2,
            max_attempts: 3,
        },
    );

    // Un comportamiento que causa otro, que causa otro: un ciclo.
    let caused = Event::new(
        "ev-caused",
        EventKind::DecisionTaken,
        serde_json::json!({}),
        Utc::now(),
    );
    runtime.register(CountingBehavior::causing(
        "chain",
        Subscription::to(EventKind::VulnerabilityAccepted),
        caused.clone(),
    ));

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");

    assert!(
        report.cut.is_some(),
        "the budget cut the chain: {:?}",
        report.cut
    );
    let cut = report.cut.expect("the cut");
    assert!(cut.contains("budget"), "and it says why: {cut}");

    // Y el corte queda en la traza, con su registro.
    let records = store.executions().await;
    assert!(
        records
            .iter()
            .any(|record| matches!(record.outcome, ExecutionOutcome::Cut { .. })),
        "and there is a record of the cut"
    );
}

// ───────────────────────────── AC4 ─────────────────────────────

/// Los efectos secundarios son idempotentes, y repetir la reaccion no
/// duplica el efecto.
#[tokio::test]
async fn the_effects_are_idempotent_and_the_repetition_does_not_duplicate_them() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());

    let behaviour = CountingBehavior::new(
        "counting",
        Subscription::to(EventKind::VulnerabilityAccepted),
    );
    runtime.register(behaviour.clone());

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );

    let first = runtime
        .dispatch(event.clone(), Utc::now())
        .await
        .expect("dispatch");
    assert_eq!(first.applied.len(), 1);
    assert_eq!(first.suppressed.len(), 0);

    // El mismo evento, otra vez.
    let second = runtime.dispatch(event, Utc::now()).await.expect("dispatch");
    assert_eq!(second.applied.len(), 0, "the effect is not applied twice");
    assert_eq!(second.duplicates.len(), 1, "and the repetition is reported");
    assert_eq!(
        second.suppressed.len(),
        0,
        "the effect is not produced again: the repetition is not run"
    );
    assert_eq!(
        behaviour.calls(),
        1,
        "and the behaviour did not run the second time"
    );

    // Y en el almacen hay un solo efecto.
    assert_eq!(store.applied_effects().await.len(), 1);

    // Y un evento DISTINTO que produce el mismo efecto lo suprime, en vez
    // de aplicarlo dos veces: la clave es la del efecto, no la del evento.
    let other = Event::new(
        "ev-2",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );
    let third = runtime.dispatch(other, Utc::now()).await.expect("dispatch");
    assert_eq!(third.applied.len(), 0);
    assert_eq!(
        third.suppressed.len(),
        1,
        "the effect was already there and it is not applied twice"
    );
    assert_eq!(store.applied_effects().await.len(), 1);
}

// ───────────────────────────── AC5 ─────────────────────────────

/// Un CVE nuevo sobre un artefacto desplegado dispara la reevaluacion y, si
/// procede, el caso.
#[tokio::test]
async fn a_new_vulnerability_triggers_the_reevaluation_and_the_case() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store, Budget::default());

    let decisions = RecordingDecisions::new();
    let cases = StubCases {
        opened: Arc::new(Mutex::new(Vec::new())),
    };
    let impacted = vec![
        ImpactedSubject {
            subject: "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web".to_string(),
            deployment: Some("production".to_string()),
            depth: 2,
        },
        ImpactedSubject {
            subject: "hrn:hodei:artifacts:eu-west-1:acme:Artifact/api".to_string(),
            deployment: Some("production".to_string()),
            depth: 1,
        },
    ];
    let radius = Arc::new(StubRadius { impacted });
    let reaction = VulnerabilityReaction::new(radius, Arc::new(cases.clone()), decisions.clone());
    runtime.register(Arc::new(reaction));

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1", "account": "acme"}),
        Utc::now(),
    );
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");

    // La reevaluacion se pidio para lo que la vulnerabilidad alcanza.
    let asked = decisions
        .asked
        .lock()
        .expect("the recorder is not poisoned");
    assert_eq!(asked.len(), 2);
    assert!(
        asked
            .iter()
            .all(|(_, reason)| *reason == ReasonCode::VulnerabilityImpacted)
    );

    // Y el caso se abrio.
    let opened = cases.opened.lock().expect("the recorder is not poisoned");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0], "GHSA-1");

    // Y los efectos son uno por sujeto.
    assert_eq!(report.applied.len(), 2);
}

/// Y una vulnerabilidad que no toca nada no reevalua nada.
#[tokio::test]
async fn a_vulnerability_that_touches_nothing_reevaluates_nothing() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store, Budget::default());

    let decisions = RecordingDecisions::new();
    runtime.register(Arc::new(VulnerabilityReaction::new(
        Arc::new(StubRadius {
            impacted: Vec::new(),
        }),
        Arc::new(StubCases {
            opened: Arc::new(Mutex::new(Vec::new())),
        }),
        decisions.clone(),
    )));

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1", "account": "acme"}),
        Utc::now(),
    );
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");

    assert!(
        decisions
            .asked
            .lock()
            .expect("the recorder is not poisoned")
            .is_empty(),
        "no reevaluation for unrelated subjects"
    );
    assert!(report.applied.is_empty());
}

// ───────────────────────────── AC7 ─────────────────────────────

/// La reconstruccion no repite los efectos externos.
#[tokio::test]
async fn the_replay_does_not_repeat_the_external_effect() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store.clone(), Budget::default());

    let behaviour = CountingBehavior::new(
        "counting",
        Subscription::to(EventKind::VulnerabilityAccepted),
    );
    runtime.register(behaviour.clone());

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );

    // La primera vez, el efecto pasa.
    let first = runtime
        .dispatch(event.clone(), Utc::now())
        .await
        .expect("dispatch");
    assert_eq!(first.applied.len(), 1);
    assert_eq!(behaviour.calls(), 1);

    // La reconstruccion lee lo que paso y NO lo repite.
    let replayed = runtime.replay(event, Utc::now()).await.expect("replay");
    assert_eq!(
        replayed.applied.len(),
        0,
        "the external effect is not executed again"
    );
    assert_eq!(replayed.duplicates.len(), 1);
    assert_eq!(behaviour.calls(), 1, "and the behaviour does not run again");
    assert_eq!(
        store.applied_effects().await.len(),
        1,
        "and there is still one effect"
    );
}

// ───────────────────────────── AC6 ─────────────────────────────

/// La caducidad de una excepcion dispara su reaccion.
#[tokio::test]
async fn the_expiry_of_an_exception_triggers_its_reaction() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store, Budget::default());

    let decisions = RecordingDecisions::new();
    runtime.register(Arc::new(ExceptionExpiryReaction::new(decisions.clone())));

    let event = Event::new(
        "ev-1",
        EventKind::ExceptionExpired,
        serde_json::json!({"subject": "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web"}),
        Utc::now(),
    );
    let report = runtime.dispatch(event, Utc::now()).await.expect("dispatch");

    let asked = decisions
        .asked
        .lock()
        .expect("the recorder is not poisoned");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1, ReasonCode::ExceptionExpired);
    assert_eq!(report.applied.len(), 1);
}

/// Y la conclusion del SPIKE-006 esta escrita, con su razon.
#[test]
fn the_conclusion_of_spike_006_is_written_down() {
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("idempotencia por clave"),
        "the conclusion says what a plain handler does not have"
    );
    assert!(
        source.contains("observable, acotado, idempotente y reconstruible"),
        "and it quotes the exit gate of the milestone"
    );
}

/// Y el informe de un despacho cuenta lo que paso, para el que lo lee.
#[tokio::test]
async fn the_report_says_what_happened() {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let mut runtime = ReactiveRuntime::new(store, Budget::default());
    runtime.register(CountingBehavior::new(
        "counting",
        Subscription::to(EventKind::VulnerabilityAccepted).where_(PayloadPredicate::FieldExists {
            field: "advisory".to_string(),
        }),
    ));

    let event = Event::new(
        "ev-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "GHSA-1"}),
        Utc::now(),
    );
    let report: DispatchReport = runtime.dispatch(event, Utc::now()).await.expect("dispatch");
    assert_eq!(report.executions.len(), 1);
    assert!(report.cut.is_none());
    assert_eq!(report.caused.len(), 0);
}
