//! The reachability of the code in Hodei.
//!
//! A CVE in a dependency is not a CVE in the code the product runs. This
//! crate is the part of Hodei that tells the two apart -- but only when
//! there is **evidence** to tell them apart with, and it says `UNKNOWN`
//! when there is not.
//!
//! # The rule that holds it all
//!
//! **Anything the analyser could not resolve is `UNKNOWN`, never
//! `UNREACHABLE`.**
//!
//! A dynamic dispatch that was not followed is not a proof that the code
//! is not reached; it is a proof that nobody knows. And an `UNREACHABLE`
//! without an explicit coverage and explicit limits is a guess, and Hodei
//! treats a guess as `UNKNOWN`.
//!
//! The consequence is that this crate never **reduces** the uncertainty
//! of a finding. It can only add certainty, and the certainty it adds
//! always comes with the name of the analyser that produced it, the
//! version of that analyser, and what it could not resolve.
//!
//! # El contrato no presupone ningun analizador
//!
//! The contract does not mention any analyser. It says what an evidence
//! has to carry so that a reader can judge it, and the port is what lets
//! any analyser enter by an adapter. The local provider that lives here
//! is bounded on purpose: it demonstrates the chain and it does not
//! pretend to be more.

#![forbid(unsafe_code)]

pub mod derivation;
pub mod evidence;
pub mod identity;
pub mod provider;
pub mod store;

pub use derivation::{Reachability, derive, shortest_path};
pub use evidence::{
    AnalyzerIdentity, CallEdge, Coverage, Limitation, ReachabilityEvidence, ReachabilityState,
};
pub use identity::{CodeIdentity, VulnerableSymbol};
pub use provider::{
    AnalysisRequest, AnalysisResult, LocalProvider, ReachabilityProvider, evidence_of,
};
pub use store::{InMemoryReachabilityStore, ReachabilityStore, ReachabilityStoreError};
