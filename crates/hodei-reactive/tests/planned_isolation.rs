//! The isolation of the planned path, with spy sinks.
//!
//! The seam is provable, not inferable: whatever fails while PLANNING --
//! the validation, the preparation, the budget -- leaves the executor
//! without a single call. And the plan is recorded BEFORE the executor
//! touches: the spy checks the order from inside.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use hodei_reactive::behavior::{BehaviorError, GuardOutcome};
use hodei_reactive::events::{Event, EventKind, Subscription};
use hodei_reactive::execution::Budget;
use hodei_reactive::intent::{Intent, IntentExecutor, Plan, PlanningBehavior, ReadScope};
use hodei_reactive::runtime::{ExecutionStore, InMemoryExecutionStore, ReactiveRuntime};

/// The spy executor: it counts every touch, and it checks, from INSIDE,
/// that the plan of the behaviour was recorded before it was asked to
/// touch.
#[derive(Clone)]
struct SpyExecutor {
    calls: Arc<Mutex<Vec<String>>>,
    store: Arc<dyn ExecutionStore>,
}

impl SpyExecutor {
    fn over(store: Arc<dyn ExecutionStore>) -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                calls: calls.clone(),
                store,
            }),
            calls,
        )
    }
}

#[async_trait::async_trait]
impl IntentExecutor for SpyExecutor {
    async fn apply(&self, intent: &Intent) -> Result<(), String> {
        // The order is the seam: said first, done after. If this
        // assertion breaks, the runtime is touching before recording, and
        // a crash in between would lose the intent.
        let key = format!(
            "planned:{}:{}",
            intent.origin.behavior, intent.origin.event_id
        );
        let record = self.store.execution_of(&key).await.expect("el plan esta dicho");
        assert!(
            record.trace.iter().any(|entry| entry.what == format!("intent:{}", intent.identity)),
            "el intent {} esta en la traza del plan antes de tocarse",
            intent.identity
        );

        self.calls
            .lock()
            .expect("el espia no esta envenenado")
            .push(intent.identity.clone());
        Ok(())
    }
}

/// Why the test behaviour fails while planning, if it fails.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// It plans two intents and one caused event.
    Plans,
    /// The validation of the plan fails: a permanent failure.
    InvalidInput,
    /// The preparation -- the read of what it needs -- fails.
    CannotRead,
}

/// The test behaviour that plans. Its read port is a function that fails
/// on demand: the behaviour does not touch any sink, whatever it reads.
struct SpyPlanning {
    mode: Mode,
}

impl SpyPlanning {
    fn with(mode: Mode) -> Arc<Self> {
        Arc::new(Self { mode })
    }
}

/// The read the behaviour needs while planning. Failing it is the
/// preparation failure.
async fn the_read(mode: Mode) -> Result<&'static str, BehaviorError> {
    match mode {
        Mode::Plans | Mode::InvalidInput => Ok("the facts of the risk"),
        Mode::CannotRead => Err(BehaviorError::transient("the read of the facts failed")),
    }
}

#[async_trait::async_trait]
impl PlanningBehavior for SpyPlanning {
    fn name(&self) -> &str {
        "spy_planning"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted)
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        if event.payload.get("advisory").is_some() {
            GuardOutcome::Pass
        } else {
            GuardOutcome::Skip("the event does not name a vulnerability".to_string())
        }
    }

    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError> {
        // The validation failure: the input does not say what it must.
        if self.mode == Mode::InvalidInput {
            return Err(BehaviorError::permanent(
                "the advisory does not name a subject",
            ));
        }

        // The preparation: the read, through the authorized scope. NO
        // sink is touched here.
        let facts = the_read(self.mode).await?;

        let first = Intent::new(
            self.name(),
            event,
            &scope.causation,
            scope.tenant.clone(),
            "security_case",
            format!("case:{}:{}", scope.event_id, event.happened_at.timestamp()),
            facts,
            serde_json::json!({"advisory": event.payload["advisory"]}),
        );
        let second = Intent::new(
            self.name(),
            event,
            &scope.causation,
            scope.tenant.clone(),
            "decision",
            format!("decision:{}:subject-1", scope.event_id),
            facts,
            serde_json::json!({"subject": "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app"}),
        );

        Ok(Plan {
            intents: vec![first, second],
            events: vec![Event::new(
                format!("evt-caused:{}", scope.event_id),
                EventKind::DecisionTaken,
                serde_json::json!({"subject": "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app"}),
                event.happened_at,
            )
            .caused_by(event.id.clone())],
        })
    }
}

/// The event that starts everything.
fn the_event() -> Event {
    Event::new(
        "evt-plan-1",
        EventKind::VulnerabilityAccepted,
        serde_json::json!({"advisory": "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-1", "account": "acme"}),
        Utc::now(),
    )
}

/// A runtime with the spy planning and the spy executor, over the same
/// store: the executor reads the store to check the order.
fn runtime_with(
    mode: Mode,
    budget: Budget,
) -> (ReactiveRuntime, Arc<Mutex<Vec<String>>>, Arc<InMemoryExecutionStore>) {
    let store = Arc::new(InMemoryExecutionStore::empty());
    let (executor, calls) = SpyExecutor::over(store.clone());
    let mut runtime = ReactiveRuntime::new(store.clone(), budget).with_tenant("acme");
    runtime.register_planned(SpyPlanning::with(mode), executor);
    (runtime, calls, store)
}

/// The plan is recorded BEFORE the executor touches, the intents are
/// applied once each, and the events the plan causes go into the chain.
#[tokio::test]
async fn the_plan_is_recorded_before_the_executor_touches() {
    let (runtime, calls, store) = runtime_with(Mode::Plans, Budget::default());

    let report = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el despacho planificado corre");

    assert_eq!(calls.lock().unwrap().len(), 2, "dos intents, dos toques");
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.caused.len(), 1, "el evento causado sale al informe");

    let record = store
        .execution_of("planned:spy_planning:evt-plan-1").await
        .expect("la ejecucion del plan queda");
    assert!(
        record
            .trace
            .iter()
            .any(|entry| entry.what.contains("the plan says 2 intents and 1 events")),
        "el plan queda dicho en la traza"
    );
}

/// A validation failure in the planning phase leaves the executor with
/// ZERO calls: the plan said nothing, so nothing is done.
#[tokio::test]
async fn a_validation_failure_touches_nothing() {
    let (runtime, calls, store) = runtime_with(Mode::InvalidInput, Budget::default());

    let report = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el despacho corre; el fallo es del comportamiento");

    assert!(
        calls.lock().unwrap().is_empty(),
        "un fallo de validacion no toca: {:?}",
        calls.lock().unwrap()
    );
    assert!(report.applied.is_empty() && report.caused.is_empty());

    let record = store
        .execution_of("planned:spy_planning:evt-plan-1").await
        .expect("el fallo queda escrito");
    match record.outcome {
        hodei_reactive::execution::ExecutionOutcome::Failed { retryable, .. } => {
            assert!(!retryable, "una validacion que falla no mejora con reintentos");
        }
        other => panic!("el resultado tiene que ser un fallo: {other:?}"),
    }
}

/// A preparation failure -- the read that failed -- also touches nothing,
/// and it is retryable: the moment may fix it.
#[tokio::test]
async fn a_preparation_failure_touches_nothing() {
    let (runtime, calls, store) = runtime_with(Mode::CannotRead, Budget::default());

    let _report = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el despacho corre; el fallo es de la lectura");

    assert!(
        calls.lock().unwrap().is_empty(),
        "un fallo de preparacion no toca: {:?}",
        calls.lock().unwrap()
    );

    let record = store
        .execution_of("planned:spy_planning:evt-plan-1").await
        .expect("el fallo queda escrito");
    match record.outcome {
        hodei_reactive::execution::ExecutionOutcome::Failed { retryable, .. } => {
            assert!(retryable, "una lectura que falla puede volver a intentarse");
        }
        other => panic!("el resultado tiene que ser un fallo: {other:?}"),
    }
}

/// An exhausted budget cuts BEFORE anything is planned, and nothing is
/// touched: el presupuesto que llega tarde ya gasto lo que cortaba.
#[tokio::test]
async fn an_exhausted_budget_touches_nothing() {
    let (runtime, calls, store) = runtime_with(
        Mode::Plans,
        Budget {
            max_executions: 0,
            max_depth: 8,
            max_attempts: 3,
        },
    );

    let report = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el despacho corre y se corta");

    assert!(calls.lock().unwrap().is_empty(), "sin presupuesto no hay toques");
    assert!(report.applied.is_empty());
    assert!(report.cut.is_some(), "el corte queda en el informe");

    let record = store.execution_of("cut:evt-plan-1").await.expect("el corte queda");
    assert!(
        matches!(
            record.outcome,
            hodei_reactive::execution::ExecutionOutcome::Cut { .. }
        ),
        "el corte es un resultado propio, no un silencio"
    );
}

/// The same event delivered twice plans once and touches once: the
/// second delivery is a duplicate whose effect is suppressed, not
/// re-applied.
#[tokio::test]
async fn an_intent_is_applied_once_only() {
    let (runtime, calls, _store) = runtime_with(Mode::Plans, Budget::default());

    let first = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el primer despacho corre");
    let second = runtime
        .dispatch_planned(the_event(), Utc::now())
        .await
        .expect("el segundo despacho corre");

    assert_eq!(first.applied.len(), 2);
    assert!(second.applied.is_empty(), "el efecto ya aplicado no se repite");
    assert!(!second.duplicates.is_empty(), "la segunda entrega es duplicado");
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "el executor toco dos veces, una por intent, ni una mas"
    );
}
