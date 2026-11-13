//! The behaviours: what reacts to what, and what it does when it reacts.
//!
//! A behaviour is **code**, not a rule written in a language: it is
//! registered by the host and the runtime calls it. The runtime handles
//! the subscription, the guard, the trace, the idempotency, the retry and
//! the budget; the behaviour handles its own work.
//!
//! That division is what keeps the runtime SMALL. There is no workflow
//! language, no dynamic plugin and no scheduler: there is a registry, a
//! trace and a budget.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{Event, Subscription};

/// What the behaviour decided when it looked at the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardOutcome {
    /// The condition holds, and the behaviour runs.
    Pass,
    /// The condition does not hold, and the behaviour does not run.
    ///
    /// The reason is in the trace: a behaviour that does not run and does
    /// not say why is a behaviour that cannot be reasoned about.
    Skip(String),
}

impl GuardOutcome {
    /// Whether the behaviour runs.
    pub fn runs(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// An effect that the reaction wants to have.
///
/// The effect carries the key that makes it **once-only**: the runtime
/// records the key, and a reaction that repeats finds the key and does
/// not repeat the effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    /// The kind of the effect: what it is.
    pub kind: String,
    /// The key that makes it once-only.
    pub idempotency_key: String,
    /// What the effect carries.
    pub payload: serde_json::Value,
}

impl Effect {
    /// An effect with its key.
    pub fn new(
        kind: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            kind: kind.into(),
            idempotency_key: idempotency_key.into(),
            payload,
        }
    }
}

/// The events that a reaction causes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    /// The events the reaction causes.
    pub events: Vec<Event>,
    /// The effects the reaction wants to have.
    pub effects: Vec<Effect>,
}

/// What went wrong when a behaviour ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorError {
    /// What went wrong.
    pub reason: String,
    /// Whether trying again can work.
    ///
    /// A failure that can be retried is a failure of the moment: the
    /// network, a sink that is down. A failure that cannot is a failure
    /// of the input: retrying it would be retrying the same mistake.
    pub retryable: bool,
}

impl BehaviorError {
    /// A failure that trying again can fix.
    pub fn transient(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            retryable: true,
        }
    }

    /// A failure that trying again cannot fix.
    pub fn permanent(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            retryable: false,
        }
    }
}

/// What the behaviour did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorOutcome {
    /// It ran and it did what it had to do.
    Completed(Reaction),
    /// It did not run.
    Skipped(String),
    /// It ran and it failed.
    Failed(BehaviorError),
}

/// A behaviour: something that reacts to an event.
///
/// **Transitional.** `act` reads and touches in one go: it is the path
/// the M6 reactions run on, and it keeps working, traced by
/// `ReactiveRuntime::dispatch`. The reactions migrate to
/// [`crate::intent::PlanningBehavior`], whose plan is said before it is
/// done; there is no silent path, whichever of the two runs.
#[async_trait::async_trait]
pub trait Behavior: Send + Sync {
    /// The name of the behaviour.
    ///
    /// It is in the trace, and a trace that does not name the behaviour
    /// that ran cannot be read.
    fn name(&self) -> &str;

    /// What the behaviour is interested in.
    fn subscription(&self) -> Subscription;

    /// Whether the condition of the behaviour holds.
    fn guard(&self, event: &Event) -> GuardOutcome;

    /// The work of the behaviour.
    ///
    /// It is async because the work of a reaction is to ask something to
    /// the graph or to the store, and a behaviour that could not ask
    /// anything would be a behaviour that decides from what it already
    /// knows.
    async fn act(&self, event: &Event) -> Result<Reaction, BehaviorError>;
}

/// One entry of the trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEntry {
    /// What happened.
    pub what: String,
    /// When it happened.
    pub at: DateTime<Utc>,
}

/// The trace of an execution.
///
/// It is **durable**: it says what the reaction tried to do, what it
/// managed and what it did not, and it survives a restart. A reaction
/// whose trace is lost is a reaction that cannot be explained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// The entries of the trace, oldest first.
    pub entries: Vec<TraceEntry>,
}

impl ExecutionTrace {
    /// A trace with nothing in it.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The trace says something.
    pub fn note(mut self, what: impl Into<String>, at: DateTime<Utc>) -> Self {
        self.entries.push(TraceEntry {
            what: what.into(),
            at,
        });
        self
    }

    /// Whether the trace says something.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the trace mentions something.
    pub fn mentions(&self, what: &str) -> bool {
        self.entries.iter().any(|entry| entry.what.contains(what))
    }
}
