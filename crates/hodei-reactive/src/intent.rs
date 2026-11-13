//! The intents: what a reaction WANTS to do, said before doing it.
//!
//! The seam is the one the trust work needs: **planning is reading and
//! saying; executing is touching**. A plan that already touches is not a
//! plan, and an execution without a plan cannot be reconciled. So the
//! reaction is split in two phases:
//!
//! - the behaviour **plans**: it reads through its authorized ports and
//!   it returns intents, and it does not touch a single sink;
//! - the runtime **records the plan first** and only then hands the
//!   intents to the executor, which is the one that touches.
//!
//! A crash between recording and touching leaves an intent that is said
//! and not done: exactly what the recovery needs to reconcile. This is
//! the seam that the durable execution store builds on.
//!
//! The legacy `Behavior` (whose `act` reads and touches in one go) keeps
//! working and keeps being traced by `ReactiveRuntime::dispatch`: it is
//! the **transitional** path while the reactions migrate, and both paths
//! leave their record, so neither is silent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::behavior::{BehaviorError, Effect, GuardOutcome};
use crate::events::{Event, Subscription};

/// The authorized read scope: what a behaviour may look at while it
/// plans, and nothing else.
///
/// The runtime hands it over; a behaviour that invents its own context is
/// deciding without an authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadScope {
    /// The tenant the plan serves. It is the adapter's own, and it is
    /// NOT the tenant a caller claims: una reclamacion no es una
    /// identidad.
    pub tenant: String,
    /// The event that triggers the plan.
    pub event_id: String,
    /// The causation chain of the event.
    pub causation: Vec<String>,
    /// The moment of the observation.
    pub at: DateTime<Utc>,
}

/// Where an intent comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentOrigin {
    /// The behaviour that planned it.
    pub behavior: String,
    /// The event that triggered it.
    pub event_id: String,
    /// The causation chain of the event, oldest first.
    pub causation: Vec<String>,
}

/// An intent: ONE external effect, said before it is done.
///
/// Everything it carries is what makes it **reconcilable**: a stable
/// identity that a restart can rebuild, the key that makes it once-only,
/// the tenant it serves, where it came from, what it was computed from,
/// and the kind of effect it is. The payload is data. **It does not
/// carry secrets**: an intent is persisted and read, and a secret that is
/// persisted is a secret that leaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// The stable identity: deterministic, so a restart rebuilds the same
    /// one and no uuid nobody can reconstruct.
    pub identity: String,
    /// The key that makes the effect once-only.
    pub idempotency_key: String,
    /// The tenant the intent serves.
    pub tenant: String,
    /// Where the intent comes from.
    pub origin: IntentOrigin,
    /// The version or observation of the input it was computed from: an
    /// intent computed from stale input says so.
    pub input_version: String,
    /// The kind of effect it wants.
    pub kind: String,
    /// What the effect carries. Data, never secrets.
    pub payload: serde_json::Value,
}

impl Intent {
    /// An intent, with everything that makes it reconcilable.
    pub fn new(
        behavior: &str,
        event: &Event,
        causation: &[String],
        tenant: impl Into<String>,
        kind: impl Into<String>,
        idempotency_key: impl Into<String>,
        input_version: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        let origin = IntentOrigin {
            behavior: behavior.to_string(),
            event_id: event.id.clone(),
            causation: causation.to_vec(),
        };
        let idempotency_key = idempotency_key.into();
        let identity = format!(
            "intent:{}:{}:{}",
            origin.behavior, origin.event_id, idempotency_key
        );
        Self {
            identity,
            idempotency_key,
            tenant: tenant.into(),
            origin,
            input_version: input_version.into(),
            kind: kind.into(),
            payload,
        }
    }

    /// The intent, said as the effect the runtime already knows how to
    /// apply once-only. Same key, same payload: one effect, whatever the
    /// path that carries it.
    pub fn as_effect(&self) -> Effect {
        Effect::new(
            self.kind.clone(),
            self.idempotency_key.clone(),
            self.payload.clone(),
        )
    }
}

/// The plan of a reaction: the intents it wants and the events it causes.
///
/// A plan does NOT touch: touching belongs to the executor. The events
/// are not external effects -- they stay in the runtime's own chain --
/// so a planning behaviour may cause them without breaking the rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The intents the reaction wants.
    pub intents: Vec<Intent>,
    /// The events the reaction causes.
    pub events: Vec<Event>,
}

/// The executor of intents: the one that TOUCHES.
///
/// The runtime records the plan first and calls the executor after; the
/// executor is the only place where an external effect happens. The
/// durable execution store builds on this seam.
#[async_trait::async_trait]
pub trait IntentExecutor: Send + Sync {
    /// Applies one intent. The once-only guarantee lives in the key: the
    /// runtime asks before calling, and the executor failing is a
    /// failure of the moment unless it says otherwise.
    async fn apply(&self, intent: &Intent) -> Result<(), String>;
}

/// A behaviour that PLANS: it reads and it says, without touching.
///
/// The legacy `Behavior::act` reads and touches in one go; this trait is
/// the separation of the two. The runtime hands over the authorized read
/// scope, the behaviour returns intents, and the sinks are only touched
/// by the executor, after the plan is recorded.
#[async_trait::async_trait]
pub trait PlanningBehavior: Send + Sync {
    /// The name of the behaviour, in the trace.
    fn name(&self) -> &str;

    /// What the behaviour is interested in.
    fn subscription(&self) -> Subscription;

    /// Whether the condition of the behaviour holds.
    fn guard(&self, event: &Event) -> GuardOutcome;

    /// The plan: what the behaviour WANTS, said before it is done.
    ///
    /// A behaviour that calls a sink here has broken the seam: the plan
    /// is reading and saying, and the failure modes are exactly the ones
    /// the spy tests ask for -- a validation failure, a preparation
    /// failure and an exhausted budget leave the sinks untouched.
    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError>;
}
