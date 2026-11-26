//! The counterfactual investigation: explaining why something is blocked
//! and what would change under a hypothesis, **without executing anything**.
//!
//! The investigator explains the world with evidence citations. The
//! simulation says what would happen if. Each on its own is one half
//! of the answer that a blocked release asks for; this crate composes
//! them into a single auditable contract, and does not add a third
//! truth: the counterfactual report **is** the investigation report
//! plus the simulation result, with the epistemic labels that
//! say where each statement comes from.
//!
//! # The labels, not the prose
//!
//! A statement of the report carries its class: what the deterministic
//! verifier verified (`verified`), what was observed with a resolvable
//! citation (`observed`), what only holds under the hypothesis
//! (`hypothetical`), what the model inferred (`inference`) and what is not
//! known (`unknown`). A recommendation is NEVER attributed to the policy
//! engine: the engine decides with rules, and what is recommended here
//! is said by whoever asks, not by the rules.
//!
//! # The shared seal
//!
//! The investigation and the simulation read the SAME read manifest
//! --[`hodei_evidence::domain::read_set`]--, and for that reason the
//! report can check that both halves speak about the same world:
//! if a source appears in both manifests with different
//! versions, the answer is rejected as an incoherent seal before
//! being published.
//!
//! # Fail closed, without leaking
//!
//! A hypothesis from another account, expired or computed against a world
//! that no longer exists is rejected with a reason that carries no foreign
//! identifiers, and the rejection happens BEFORE retrieving anything: no
//! data is leaked from the world that the hypothesis claimed to touch.

#![forbid(unsafe_code)]

pub mod report;
pub mod service;

pub use report::{
    COUNTERFACTUAL_REPORT_VERSION, CounterfactualReport, EpistemicLabel, SealCoherence, Statement,
    the_seal_coherence,
};
pub use service::{CounterfactualError, CounterfactualService, RejectionReason};
