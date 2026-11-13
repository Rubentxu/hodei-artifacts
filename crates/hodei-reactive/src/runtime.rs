//! The runtime of the reactions.
//!
//! The runtime does four things and nothing mas: **subscribe**, **guard**,
//! **record** and **bound**. It does not decide what a reaction means, and
//! it does not have a language in which to say it.
//!
//! That is the conclusion of the SPIKE-006, y se puede escribir entera:
//!
//! - **Suscribir**: un registro de comportamientos, cada uno con lo que
//!   le interesa. Es un registro y no un plan.
//! - **Guardar**: la condicion se comprueba, y si no se cumple la
//!   reaccion no corre y el motivo queda en la traza.
//! - **Registrar**: la ejecucion es durable, con su cadena de causalidad,
//!   su numero de intentos y su traza.
//! - **Acotar**: un presupuesto corta la cadena cuando se excede, y el
//!   corte queda en la traza.
//!
//! Lo que un manejador de eventos ordinario NO tiene, y por lo que el
//! runtime minimo si hace falta: la idempotencia por clave, el presupuesto
//! de bucles, la traza durable y la reconstruccion sin repetir los
//! efectos externos. Un manejador ordinario es una llamada a una funcion:
//! no tiene clave, ni presupuesto, ni traza que sobreviva a un
//! reinicio. Y la exit gate del hito lo pide literal: "observable,
//! acotado, idempotente y reconstruible".

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::behavior::{Behavior, BehaviorOutcome, Effect, Reaction, TraceEntry};
use crate::events::{Event, EventKind};
use crate::execution::ExecutionOutcome;
use crate::execution::{Budget, BudgetState, ExecutionRecord};
use crate::intent::{IntentExecutor, PlanningBehavior, ReadScope};

/// Why the runtime could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// The execution store is not available.
    Unavailable(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "the execution store is not available: {message}")
            }
        }
    }
}

/// What happened in one dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DispatchReport {
    /// The executions that happened.
    pub executions: Vec<ExecutionRecord>,
    /// The effects that were applied.
    pub applied: Vec<Effect>,
    /// The effects that were NOT applied because they had already been.
    pub suppressed: Vec<Effect>,
    /// The events the reactions caused.
    pub caused: Vec<Event>,
    /// The chain was cut, and this is why.
    pub cut: Option<String>,
    /// The events that were delivered and were already known.
    pub duplicates: Vec<String>,
}

impl DispatchReport {
    /// The execution of a behaviour.
    pub fn execution_of(&self, behavior: &str) -> Option<&ExecutionRecord> {
        self.executions
            .iter()
            .find(|execution| execution.behavior == behavior)
    }
}

/// The store of the executions.
///
/// The store is what makes the reaction **once-only** and what makes the
/// trace **durable**. A reaction whose record is not there has not
/// happened, whatever anybody remembers.
pub trait ExecutionStore: Send + Sync {
    /// The execution of a key, if it happened.
    fn execution_of(&self, key: &str) -> Option<ExecutionRecord>;

    /// Writes an execution down.
    fn put(&self, record: ExecutionRecord);

    /// The effects that have already been applied.
    fn applied_effects(&self) -> Vec<Effect>;

    /// Records that an effect was applied.
    fn record_effect(&self, effect: Effect);

    /// The executions of a chain, the oldest first.
    fn executions(&self) -> Vec<ExecutionRecord>;
}

/// The store of the executions, in memory.
#[derive(Debug, Clone, Default)]
pub struct InMemoryExecutionStore {
    inner: Arc<std::sync::Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    executions: Vec<ExecutionRecord>,
    effects: Vec<Effect>,
}

impl InMemoryExecutionStore {
    /// A store with nothing in it.
    pub fn empty() -> Self {
        Self::default()
    }
}

impl ExecutionStore for InMemoryExecutionStore {
    fn execution_of(&self, key: &str) -> Option<ExecutionRecord> {
        let inner = self.inner.lock().expect("the store is not poisoned");
        inner
            .executions
            .iter()
            .find(|execution| execution.key == key)
            .cloned()
    }

    fn put(&self, record: ExecutionRecord) {
        let mut inner = self.inner.lock().expect("the store is not poisoned");
        inner
            .executions
            .retain(|existing| existing.key != record.key);
        inner.executions.push(record);
    }

    fn applied_effects(&self) -> Vec<Effect> {
        let inner = self.inner.lock().expect("the store is not poisoned");
        inner.effects.clone()
    }

    fn record_effect(&self, effect: Effect) {
        let mut inner = self.inner.lock().expect("the store is not poisoned");
        if !inner
            .effects
            .iter()
            .any(|existing| existing.idempotency_key == effect.idempotency_key)
        {
            inner.effects.push(effect);
        }
    }

    fn executions(&self) -> Vec<ExecutionRecord> {
        let inner = self.inner.lock().expect("the store is not poisoned");
        inner.executions.clone()
    }
}

/// The runtime of the reactions.
pub struct ReactiveRuntime {
    behaviors: Vec<Arc<dyn Behavior>>,
    store: Arc<dyn ExecutionStore>,
    budget: Budget,
    /// The behaviours that PLAN instead of touching. The legacy
    /// `behaviors` and this registry are both traced: neither is a
    /// silent path.
    planners: Vec<Arc<dyn PlanningBehavior>>,
    /// The one that touches, after the plan is recorded.
    executor: Option<Arc<dyn IntentExecutor>>,
    /// The tenant of the runtime: the authority of the planned path. It
    /// comes from the composition, and NOT from an event payload, because
    /// an event is a claim and not an identity.
    planned_tenant: Option<String>,
}

impl ReactiveRuntime {
    /// A runtime over a store and a budget.
    pub fn new(store: Arc<dyn ExecutionStore>, budget: Budget) -> Self {
        Self {
            behaviors: Vec::new(),
            store,
            budget,
            planners: Vec::new(),
            executor: None,
            planned_tenant: None,
        }
    }

    /// The tenant of the planned path, from the composition.
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.planned_tenant = Some(tenant.into());
        self
    }

    /// Registers a behaviour.
    pub fn register(&mut self, behavior: Arc<dyn Behavior>) {
        self.behaviors.push(behavior);
    }

    /// Registers a behaviour that PLANS, and the executor that touches
    /// for it. Without both, the planned path does not start: un plan sin
    /// quien lo haga dicho es un plan que nadie cumple.
    pub fn register_planned(
        &mut self,
        planner: Arc<dyn PlanningBehavior>,
        executor: Arc<dyn IntentExecutor>,
    ) {
        self.planners.push(planner);
        self.executor = Some(executor);
    }

    /// The behaviours that are registered.
    pub fn behaviors(&self) -> &[Arc<dyn Behavior>] {
        &self.behaviors
    }

    /// The store of the executions.
    pub fn store(&self) -> &Arc<dyn ExecutionStore> {
        &self.store
    }

    /// Delivers an event and runs what reacts to it.
    ///
    /// The chain is walked in breadth, with the budget counting, and every
    /// execution is written down before the effects are applied: a crash
    /// between the two leaves a record of an intent, and a reaction that
    /// repeats finds the record and does not repeat the effect.
    pub async fn dispatch(
        &self,
        event: Event,
        now: DateTime<Utc>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.dispatch_inner(event, now, false).await
    }

    /// Replays a chain from the durable records, without executing
    /// anything.
    ///
    /// The replay is what reconstructs the state after a restart: it
    /// reads what happened and it does **not** do it again. Un efecto
    /// externo que ya se aplico no se vuelve a aplicar, y el resultado
    /// que se grabo es el que se usa.
    pub async fn replay(
        &self,
        event: Event,
        now: DateTime<Utc>,
    ) -> Result<DispatchReport, RuntimeError> {
        self.dispatch_inner(event, now, true).await
    }

    /// Delivers an event to the behaviours that PLAN.
    ///
    /// The order is the seam: **plan, record, then touch**. The planner
    /// reads through its authorized scope and returns intents; the intents
    /// are written down in the execution record BEFORE the executor is
    /// called, so a crash in between leaves an intent that is said and not
    /// done -- what the recovery reconciles. The executor is the only one
    /// that touches, and its once-only guarantee is the same key the
    /// legacy effects use, so there is ONE idempotency, not two.
    pub async fn dispatch_planned(
        &self,
        event: Event,
        now: DateTime<Utc>,
    ) -> Result<DispatchReport, RuntimeError> {
        let Some(executor) = self.executor.clone() else {
            return Err(RuntimeError::Unavailable(
                "no executor of intents is registered".to_string(),
            ));
        };
        let Some(tenant) = self.planned_tenant.clone() else {
            return Err(RuntimeError::Unavailable(
                "the planned path needs the tenant of the composition, and there is none"
                    .to_string(),
            ));
        };

        let mut report = DispatchReport::default();
        let mut state = BudgetState::new(self.budget.clone());
        let mut pending: Vec<(Event, Vec<String>)> = vec![(event.clone(), vec![event.id.clone()])];

        while let Some((current, causation)) = pending.pop() {
            // The budget is checked BEFORE anything happens: un corte que
            // llega tarde ya gasto lo que cortaba.
            if let Some(reason) = state.is_exhausted(0) {
                report.cut = Some(reason.clone());
                let record = ExecutionRecord::new(
                    format!("cut:{}", current.id),
                    "runtime",
                    current.id.clone(),
                    causation.clone(),
                    now,
                )
                .finished(ExecutionOutcome::Cut {
                    reason: reason.clone(),
                }, now);
                self.store.put(record.clone());
                report.executions.push(record);
                break;
            }
            state.consume();

            for planner in &self.planners {
                if !planner.subscription().matches(&current) {
                    continue;
                }

                let key = format!("planned:{}:{}", planner.name(), current.id);

                // Once-only: the execution is looked for before anything
                // is planned.
                if let Some(previous) = self.store.execution_of(&key) {
                    match previous.outcome {
                        ExecutionOutcome::Completed => {
                            report.duplicates.push(current.id.clone());
                            let record =
                                previous.clone().finished(ExecutionOutcome::Duplicate, now);
                            report.executions.push(record);
                        }
                        _ => {
                            report.duplicates.push(current.id.clone());
                            report.executions.push(previous);
                        }
                    }
                    continue;
                }

                // The guard.
                let mut record = ExecutionRecord::new(
                    key.clone(),
                    planner.name(),
                    current.id.clone(),
                    causation.clone(),
                    now,
                );
                match planner.guard(&current) {
                    crate::behavior::GuardOutcome::Skip(reason) => {
                        record = record.finished(ExecutionOutcome::Skipped { reason }, now);
                        self.store.put(record.clone());
                        report.executions.push(record);
                        continue;
                    }
                    crate::behavior::GuardOutcome::Pass => {
                        record = record.note("the guard passed", now);
                    }
                }

                // The plan. This is the phase that MUST NOT touch: what
                // the planner can do here is read and say, and the spy
                // tests are the proof of the cut.
                let scope = ReadScope {
                    tenant: tenant.clone(),
                    event_id: current.id.clone(),
                    causation: causation.clone(),
                    at: now,
                };
                match planner.plan(&current, &scope).await {
                    Ok(plan) => {
                        record = record.note(
                            format!(
                                "the plan says {} intents and {} events",
                                plan.intents.len(),
                                plan.events.len()
                            ),
                            now,
                        );
                        for intent in &plan.intents {
                            record = record.note(format!("intent:{}", intent.identity), now);
                        }
                        for caused in &plan.events {
                            record = record.note(format!("caused:{}", caused.id), now);
                        }
                        record = record.finished(ExecutionOutcome::Completed, now);

                        // The plan is recorded BEFORE the executor is
                        // called: said first, done after.
                        self.store.put(record.clone());
                        report.executions.push(record);

                        for intent in plan.intents {
                            let already = self
                                .store
                                .applied_effects()
                                .iter()
                                .any(|applied| {
                                    applied.idempotency_key == intent.idempotency_key
                                });
                            if already {
                                report.suppressed.push(intent.as_effect());
                                continue;
                            }
                            match executor.apply(&intent).await {
                                Ok(()) => {
                                    self.store.record_effect(intent.as_effect());
                                    report.applied.push(intent.as_effect());
                                }
                                Err(reason) => {
                                    // The intent is said and not done:
                                    // the failed apply is in the store,
                                    // and the recovery reconciles.
                                    let failed = ExecutionRecord::new(
                                        format!("apply:{}", intent.identity),
                                        planner.name(),
                                        current.id.clone(),
                                        causation.clone(),
                                        now,
                                    )
                                    .note(format!("intent:{}", intent.identity), now)
                                    .finished(
                                        ExecutionOutcome::Failed {
                                            reason: format!(
                                                "the intent {} could not be applied: {}",
                                                intent.identity, reason
                                            ),
                                            retryable: true,
                                        },
                                        now,
                                    );
                                    self.store.put(failed.clone());
                                    report.executions.push(failed);
                                }
                            }
                        }

                        // And the events the plan causes.
                        for caused in &plan.events {
                            pending.push((caused.clone(), causation_of(&causation, &caused.id)));
                            report.caused.push(caused.clone());
                        }
                    }
                    Err(error) => {
                        record = record
                            .note(format!("the planning failed: {}", error.reason), now)
                            .finished(
                                ExecutionOutcome::Failed {
                                    reason: error.reason,
                                    retryable: error.retryable,
                                },
                                now,
                            );
                        self.store.put(record.clone());
                        report.executions.push(record);
                    }
                }
            }

            state.deepen();
            if pending.is_empty() {
                break;
            }
            state.shallow();
        }

        Ok(report)
    }

    /// Delivers an event and runs what reacts to it.
    async fn dispatch_inner(
        &self,
        event: Event,
        now: DateTime<Utc>,
        replaying: bool,
    ) -> Result<DispatchReport, RuntimeError> {
        let mut report = DispatchReport::default();
        let mut state = BudgetState::new(self.budget.clone());
        // The reactions of THIS step. They are not the reactions of the
        // chain: keeping them across the steps would put the same events
        // in the queue once and again, and the chain would grow without
        // the behaviours doing anything.
        let mut reactions: Vec<Reaction> = Vec::new();
        // The effects of the whole chain: these are NOT cleared, because
        // an effect that was produced in the first step is an effect of
        // the chain.
        let mut effects: Vec<Effect> = Vec::new();

        // The chain: the events that are waiting to be delivered.
        let mut pending: Vec<(Event, Vec<String>)> = vec![(event.clone(), vec![event.id.clone()])];

        while let Some((current, causation)) = pending.pop() {
            // The budget is checked BEFORE anything happens, and the cut
            // is written down.
            if let Some(reason) = state.is_exhausted(0) {
                report.cut = Some(reason.clone());
                let record = ExecutionRecord::new(
                    format!("cut:{}", current.id),
                    "runtime",
                    current.id.clone(),
                    causation.clone(),
                    now,
                )
                .finished(
                    ExecutionOutcome::Cut {
                        reason: reason.clone(),
                    },
                    now,
                );
                self.store.put(record.clone());
                report.executions.push(record);
                break;
            }
            state.consume();

            for behavior in &self.behaviors {
                if !behavior.subscription().matches(&current) {
                    continue;
                }

                let key = format!("{}:{}", behavior.name(), current.id);

                // Once-only: the execution is looked for before anything
                // is done.
                if let Some(previous) = self.store.execution_of(&key) {
                    match previous.outcome {
                        ExecutionOutcome::Completed => {
                            report.duplicates.push(current.id.clone());
                            let record =
                                previous.clone().finished(ExecutionOutcome::Duplicate, now);
                            report.executions.push(record);
                            continue;
                        }
                        ExecutionOutcome::Failed {
                            retryable: true, ..
                        } => {
                            // The failure can be retried, and the retry
                            // starts from where the failure left it.
                            let attempts = previous.attempts;
                            if let Some(reason) = state.is_exhausted(attempts) {
                                report.cut = Some(reason.clone());
                                break;
                            }
                            let (record, reaction) = self
                                .run_behavior(behavior, &current, &causation, attempts, now)
                                .await;
                            self.store.put(record.clone());
                            report.executions.push(record);
                            effects.extend(reaction.effects.clone());
                            reactions.push(reaction);
                            continue;
                        }
                        _ => {
                            report.duplicates.push(current.id.clone());
                            report.executions.push(previous);
                            continue;
                        }
                    }
                }

                if replaying {
                    // The replay does not run anything: it says what
                    // happened, and it uses the result that was recorded.
                    let record = ExecutionRecord::new(
                        key.clone(),
                        behavior.name(),
                        current.id.clone(),
                        causation.clone(),
                        now,
                    )
                    .finished(ExecutionOutcome::Duplicate, now);
                    report.duplicates.push(current.id.clone());
                    report.executions.push(record);
                    continue;
                }

                let (record, reaction) = self
                    .run_behavior(behavior, &current, &causation, 0, now)
                    .await;
                self.store.put(record.clone());
                report.executions.push(record);
                effects.extend(reaction.effects.clone());
                reactions.push(reaction);
            }

            // And the events the reactions caused.
            let caused_by_this_step: Vec<Event> = reactions
                .iter()
                .flat_map(|reaction| reaction.events.clone())
                .collect();
            reactions.clear();
            for caused in &caused_by_this_step {
                pending.push((caused.clone(), causation_of(&causation, &caused.id)));
            }

            state.deepen();
            if pending.is_empty() {
                break;
            }
            state.shallow();
        }

        // The effects are applied at the end, once-only each. A replay
        // applies NONE: what happened already happened.
        for effect in effects {
            if replaying {
                report.suppressed.push(effect);
                continue;
            }
            let already = self
                .store
                .applied_effects()
                .iter()
                .any(|applied| applied.idempotency_key == effect.idempotency_key);
            if already {
                report.suppressed.push(effect);
            } else {
                self.store.record_effect(effect.clone());
                report.applied.push(effect);
            }
        }

        Ok(report)
    }

    /// Runs one behaviour and writes down what happened.
    async fn run_behavior(
        &self,
        behavior: &Arc<dyn Behavior>,
        event: &Event,
        causation: &[String],
        attempts: u32,
        now: DateTime<Utc>,
    ) -> (ExecutionRecord, Reaction) {
        let mut record = ExecutionRecord::new(
            format!("{}:{}", behavior.name(), event.id),
            behavior.name(),
            event.id.clone(),
            causation.to_vec(),
            now,
        );
        // The attempt that is being made is one more than the ones that
        // came before: a retry that does not count is a retry that can
        // happen for ever.
        record.attempts = attempts + 1;

        // The guard.
        match behavior.guard(event) {
            crate::behavior::GuardOutcome::Skip(reason) => {
                return (
                    record.finished(ExecutionOutcome::Skipped { reason }, now),
                    Reaction {
                        events: Vec::new(),
                        effects: Vec::new(),
                    },
                );
            }
            crate::behavior::GuardOutcome::Pass => {
                record = record.note("the guard passed", now);
            }
        }

        // The work.
        match behavior.act(event).await {
            Ok(reaction) => {
                record = record.note(
                    format!(
                        "the behaviour ran and produced {} effects and {} events",
                        reaction.effects.len(),
                        reaction.events.len()
                    ),
                    now,
                );
                for caused in &reaction.events {
                    record = record.note(format!("caused:{}", caused.id), now);
                }
                for effect in &reaction.effects {
                    record = record.note(format!("effect:{}", effect.kind), now);
                }
                (record.finished(ExecutionOutcome::Completed, now), reaction)
            }
            Err(error) => {
                record = record.note(format!("the behaviour failed: {}", error.reason), now);
                (
                    record.finished(
                        ExecutionOutcome::Failed {
                            reason: error.reason,
                            retryable: error.retryable,
                        },
                        now,
                    ),
                    Reaction {
                        events: Vec::new(),
                        effects: Vec::new(),
                    },
                )
            }
        }
    }
}

/// The causation chain of an event caused by another one.
fn causation_of(causation: &[String], caused_by: &str) -> Vec<String> {
    let mut chain = causation.to_vec();
    chain.push(caused_by.to_string());
    chain
}

/// The kinds of event the runtime knows.
pub fn known_kinds() -> Vec<EventKind> {
    vec![
        EventKind::VulnerabilityAccepted,
        EventKind::ExceptionExpired,
        EventKind::EvidenceAccepted,
        EventKind::DecisionTaken,
    ]
}

/// The trace of an execution, for a reader.
pub fn trace_of(record: &ExecutionRecord) -> &[TraceEntry] {
    &record.trace
}

/// The outcome of an execution, for a reader.
pub fn outcome_of(record: &ExecutionRecord) -> &ExecutionOutcome {
    &record.outcome
}

/// The behaviour outcome of an execution, when it is known.
pub fn behavior_outcome(record: &ExecutionRecord) -> Option<BehaviorOutcome> {
    match &record.outcome {
        ExecutionOutcome::Completed => {
            Some(BehaviorOutcome::Completed(crate::behavior::Reaction {
                events: Vec::new(),
                effects: Vec::new(),
            }))
        }
        ExecutionOutcome::Skipped { reason } => Some(BehaviorOutcome::Skipped(reason.clone())),
        ExecutionOutcome::Failed { reason, retryable } => {
            Some(BehaviorOutcome::Failed(crate::behavior::BehaviorError {
                reason: reason.clone(),
                retryable: *retryable,
            }))
        }
        _ => None,
    }
}
