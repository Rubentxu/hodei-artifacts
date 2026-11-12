//! The trust policy of Hodei.
//!
//! This crate is the domain of the trust gate: the context that a policy is
//! measured against, the policy itself, the decision it produces, the
//! registry of the activated policy sets, and the gate that puts the
//! authorization and the trust side by side without confusing them.
//!
//! It knows nothing about SurrealDB, about HTTP or about Cedar. What it
//! knows is how to take a context and a policy and say, deterministically,
//! what the decision is.
//!
//! # Where the trust policy does NOT live
//!
//! **The trust gate is not written in Cedar.** That is the conclusion of
//! SPIKE-004, and it is a conclusion about the shape of the two languages,
//! not about how good Cedar is.
//!
//! Cedar evaluates one request at a time, against a `(principal, action,
//! resource)` triple, and it answers yes or no with a reason for the no.
//! A trust policy is none of that:
//!
//! - It is a **set of requirements** over a **precomputed context**: the
//!   capsule says thirteen things about the artefact and the policy says
//!   which of them have to hold. Cedar has no aggregates, so "the SBOM is
//!   complete *and* the provenance is verified *and* no critical
//!   vulnerability is reachable" is three policies that do not know about
//!   each other, and the rule that says "the worst one wins" does not
//!   exist in Cedar.
//! - It has to produce **machine-readable reason codes** as its output.
//!   Cedar's reasons are diagnostic text about one request; the reason
//!   codes of a trust decision are a closed set that a report, a policy
//!   and an audit can all read, and they are per requirement, not per
//!   request.
//! - It is **versioned and activated with history**. Cedar's policy set is
//!   the one that is loaded; there is no notion of a set that was
//!   activated at a moment and that a decision made six months ago can
//!   still be read under.
//!
//! What Cedar *does* do, and does better than anything else here, is
//! decide **who may do what**. That is the authorization half, it stays in
//! Cedar, and it is exactly what Cedar's model is for.
//!
//! So the boundary is:
//!
//! ```text
//! Cedar                    ->  who may promote this
//! the trust policy engine  ->  whether the artefact qualifies
//! ```
//!
//! And the gate that puts them together says **which one decided**, so a
//! refusal is never a riddle.
//!
//! The trust-specific computations -- what is reachable, what is critical,
//! what is exposed -- happen **before** the evaluation and arrive as
//! facts in the context. A policy that queried a graph would be a policy
//! whose decision depends on when it was asked, and that is the opposite
//! of reproducible.

#![forbid(unsafe_code)]

pub mod context;
pub mod decision;
pub mod exception;
pub mod gate;
pub mod policy;
pub mod ports;
pub mod promotion;
pub mod reasons;
pub mod registry;
pub mod service;
pub mod simulation;

pub use context::{
    ExceptionFact, Exposure, RiskFacts, SubjectFact, TargetEnvironment, TrustContext,
    TrustContextBuilder, TRUST_CONTEXT_VERSION,
};
pub use decision::{evaluate, Outcome, RequirementOutcome, TrustDecision};
pub use exception::{ApprovalOutcome, ExceptionRequest, ExceptionState, RefusalReason};
pub use gate::{AuthorizationResult, DecidedBy, GateResult, PromotionGate};
pub use policy::{OnFailure, Requirement, RequirementKind, TrustPolicy};
pub use ports::{DecisionStore, DecisionStoreError, PolicyLoader, PromotionTargets};
pub use promotion::{
    DecisionId, PromotionDecision, PromotionFailure, PromotionOutcome, PromotionRequest,
    PromotionTarget,
};
pub use reasons::ReasonCode;
pub use registry::{Activation, PolicyRegistry};
pub use service::PromotionService;
pub use simulation::{simulate, AffectedRelease, SimulationReport};

/// The version of the schema that a policy set is written against.
///
/// A policy set says which schema it was written for, and a decision says
/// the same, so that a policy that is read with another schema's meaning
/// is caught rather than silently interpreted.
pub const POLICY_SCHEMA_VERSION: &str = "1.0.0";
