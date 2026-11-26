//! The AI investigator: an explanation over the graph that can be
//! audited.
//!
//! M8 is the most demanding milestone of the roadmap, and its demand is a single
//! thing: **an explanation with evidence citations and verifiable paths,
//! never self-authorized decisions**.
//!
//! # The architecture is the guarantee
//!
//! An AI investigator that can act is an investigator that can
//! be wrong with consequences. That is why here:
//!
//! - The **model has no way to act**. Its port returns text, and
//!   the text is **validated** before it is published. There is no path from the
//!   model to the trust decisions.
//! - What the model says is **checked with the tools**: a
//!   FACT-type claim needs a citation that resolves, and a citation
//!   that does not resolve **rejects the entire answer**.
//! - **Inference is declared as inference**, and what is not known is
//!   said to be unknown. An invented certainty is an invitation to not
//!   check anything.
//! - **Retrieval has a scope**: the account, the subject and the
//!   limits, and the scope is checked twice (when retrieving and when
//!   using), because a retriever that is wrong cannot let anything in.
//! - **Non-read actions wait for human approval**, and
//!   the approval stays in the trace with its name and its moment.
//!
//! # Third-party text is DATA
//!
//! An SBOM description is written by whoever publishes the package. A README
//! is written by whoever wants. Both are attacker text as soon as the
//! attacker is the one who publishes. That is why the context goes **delimited and
//! declared as data**, and that is why the injection regression corpus
//! exists: the delimiters are **not** the guarantee (a model can
//! obey a delimited text), the guarantee is that what the model says
//! is checked and that the model has no way to act.

// # The conclusion of SPIKE-011
//
//! The output of the spike is: *the AI functionality only proceeds if it cannot
//! alter the authoritative trust state and if the quality of its
//! grounding reaches the agreed threshold*.
//!
//! Both conditions are met **by construction**, and not by promise:
//!
//! 1. **It cannot alter the trust state.** The model has no
//!    way to act: its port returns text and the text is checked. There
//!    is no path from the model to the decisions, and the decisions are
//!    still taken by the deterministic policy engine.
//! 2. **The grounding is checked.** A FACT needs a citation that
//!    resolves, a citation that does not resolve **rejects the entire
//!    answer**, and a half answer is worse than none: it is rejected
//!    or not published.
//!
//! And there is a third condition, the one that the spike's red team
//! («an SBOM description is attacker-controlled text»)
//! left written: third-party text is delimited DATA, and the regression
//! corpus shows that evidence text does not steer the
//! answer. The delimiters are **not** the guarantee --a model can
//! obey a delimited text--: the guarantee is that what the
//! model says is checked with the tools and that the model has no way to
//! act.
//
//! The quality threshold is measured in the UAT of M8, and the provider mode
//! is fixture-based: the spike is about the **security** of the investigator, not
//! about the quality of a commercial model, and that quality cannot be
//! measured without the provider.

#![forbid(unsafe_code)]

pub mod claim;
pub mod corpus;
pub mod deterministic;
pub mod freshness;
pub mod guard;
pub mod investigator;
pub mod model;
pub mod read_set;
pub mod redact;
pub mod report;
pub mod scope;

pub use claim::{AIClaim, ClaimError, ClaimKind, InvestigationTrace};
pub use freshness::{
    ChangeKind, ChangedReference, CurrentWorld, EVIDENCE_SOURCE, Freshness, GRAPH_SOURCE, judge,
};
pub use guard::{
    ActionKind, ActionState, CapabilityGuard, GuardError, ProposedAction, TracedAction,
};
pub use investigator::{EvidenceRetriever, Investigation, InvestigationError, Investigator};
pub use model::{FixtureProvider, ModelProvider, ModelRequest, ModelResponse, PROMPT_VERSION};
pub use read_set::{ReadKind, ReadSet, ReadStamp};
pub use report::{Finding, FindingKind, InvestigationReport, VerificationStatus};
pub use scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalBudget, RetrievalError, RetrievalScope,
    RetrievedContext, ScopeViolation, ScopedFact, TrustLevel, check_the_scope,
};
