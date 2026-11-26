//! The runtime of Hodei's security reactions.
//!
//! When a new risk touches a deployed artifact, the trust decision
//! has to change **without anybody asking for it**. This crate is
//! the part of the system that finds out on its own.
//!
//! # What the runtime does, and nothing more
//!
//! - **Subscribe**: a registry of behaviours, each with what
//!   interests it.
//! - **Guard**: the condition is checked; if it does not hold, the reaction
//!   does not run and the reason stays in the trace.
//! - **Record**: the execution is durable, with its causation chain,
//!   its attempt count and its trace.
//! - **Bound**: a budget cuts the chain when it is exceeded, and the
//!   cut stays in the trace.
//!
//! # What an ordinary event handler does not have
//!
//! Idempotency by key, the loop budget, the durable trace
//! and reconstruction without repeating external effects. An ordinary
//! handler is a function call: it has no key, no budget,
//! and no trace that survives a restart. And the milestone's exit gate asks for it
//! literally: **observable, bounded, idempotent and reconstructible**.

#![forbid(unsafe_code)]

pub mod behavior;
pub mod events;
pub mod execution;
pub mod intent;
pub mod reactions;
pub mod runtime;

pub use behavior::{
    Behavior, BehaviorError, BehaviorOutcome, Effect, GuardOutcome, Reaction, TraceEntry,
};
pub use events::{Event, EventKind, PayloadPredicate, Subscription};
pub use execution::{Budget, BudgetState, ExecutionOutcome, ExecutionRecord};
pub use intent::{
    EventOrigin, InMemoryIntentOutbox, Intent, IntentExecutor, IntentOrigin, IntentOutbox,
    IntentScan, IntentStatus, IntentStatusSource, Plan, PlanningBehavior, ReadScope, Receipt,
};
pub use reactions::{
    BlastRadiusSource, DecisionRequester, DomainIntentExecutor, ExceptionExpiryReaction,
    ImpactedSubject, PlannedExceptionExpiryReaction, PlannedVulnerabilityReaction, ReasonCode,
    Reevaluation, SecurityCaseSink, VulnerabilityReaction,
};
pub use runtime::{
    DispatchReport, ExecutionStore, InMemoryExecutionStore, ReactiveRuntime, RuntimeError,
};
