//! The simulation: asking "what would happen if" without touching the truth.
//!
//! Changing a promotion policy and discovering afterwards that it broke three
//! deployments is a bad deal. Here that question is answered without
//! deploying anything: a **fork** overlaid on the real state, and a **diff**
//! that says which relations appear, which relations disappear and which
//! decisions change.
//!
//! # The SPIKE-008 decision: overlay, not copy
//!
//! SPIKE-008 compared four mechanisms:
//!
//! 1. replay from a checkpoint in an isolated namespace;
//! 2. a copy-on-write overlay on the base projection;
//! 3. a native snapshot or branch of the store;
//! 4. a checkpoint plus an overlay.
//!
//! **The overlay is chosen (2 and 4)**, and the reason is arithmetic:
//!
//! - A **copy** costs what the state costs: at the reference
//!   scale, the projection takes 156 s and the reconstruction 656.6 s. And
//!   that is per simulation, every time someone asks.
//! - An **overlay** costs what the hypothetical facts cost,
//!   which are few: the fork is a VALUE, not a copy.
//! - The **store has no portable branches**: the embedded engine locks
//!   the directory, and a store branch would be a file copy
//!   with its own pitfalls.
//!
//! And isolation is not a promise, it is a **construction**: the graph
//! port **has no write method**, and the service checks
//! at the end that the world on which it evaluated is still the same
//! one that was read.
//!
//! # The pinned base
//!
//! A simulation that does not say **on which world** it was computed is an
//! answer without a surname. That is why the diff carries its base: the account, the
//! clock, the generation of the graph, the seal of the content read and the
//! manifest of what was read --the relations, the policy and the
//! evidence consumed by the decision--.
//!
//! And that base is **checked twice**: if the world moved
//! while it was being evaluated, the simulation declares itself stale instead of
//! returning a diff on a world that no longer exists. The fork has to
//! declare which generation it was prepared against, and the simulation does not accept
//! a base without a version: a base that cannot be checked does not support
//! an answer.
//!
//! # What the diff says
//!
//! The diff resembles a git diff: you see which relations **appear** and
//! which relations **disappear**, and which **decisions change**, with the
//! version of the policy that produced them. And a decision that stays the same
//! is not a change: a diff full of noise is not a diff.
//!
//! # The limits
//!
//! A simulation is a resource, and a resource without a limit eats the
//! machine. The limits are **declared** --how many facts, how many
//! subjects, how long it lives-- and are checked **before** doing anything.
//!
//! # The versioned contract
//!
//! The diff is the interior of the simulation. The **result** is the
//! outer contract: what a consumer receives, and for that reason it is
//! versioned, separates the observed from the hypothetical, says what has been
//! covered and what has not, and declares the scenario's conflicts instead of
//! hiding them.
//!
//! And the contract travels **without policy objects**: what travels is the
//! seal of the policy --its identifier and its version--, because a
//! contract that drags the whole object along stops being portable.

#![forbid(unsafe_code)]

pub mod base;
pub mod compare;
pub mod conformance;
pub mod diff;
pub mod fork;
pub mod hypothetical;
pub mod memory;
pub mod ports;
pub mod result;
pub mod service;

pub use base::{SimulationBase, the_seal_of};
pub use compare::{CanonicalDiff, normalize_decisions, normalize_relations};
pub use conformance::{ConformanceReport, ConformanceWorld, Fixture, ScenarioResult, run_suite};
pub use diff::{DecisionChange, SimulationDiff, StructuralDiff, TheDecision};
pub use fork::{Checkpoint, SimulationFork, SimulationLimits};
pub use hypothetical::{Hypothetical, HypotheticalClaim, RelationFact, SimulationTag};
pub use memory::{InMemoryConformance, InMemoryDecisions, InMemoryGraph};
pub use ports::{DecisionEvaluator, SimulationGraph};
pub use result::{
    BaseStamp, Conflict, ConflictKind, Coverage, EvidenceStamp, Hypothesis, HypothesisKind,
    ObservedState, Omission, PolicyStamp, SIMULATION_RESULT_VERSION, SimulationResult, SourceStamp,
};
pub use service::{SimulationError, SimulationService};
