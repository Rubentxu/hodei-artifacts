//! Preventive advisories: the heuristic signals that say "this looks
//! wrong" without ever saying "this is malware".
//!
//! The vulnerability feed reads what the publishers say about known
//! problems. A preventive advisory is the other half: it reads what the
//! package itself says about itself -- its name, its publisher, its
//! metadata -- and it flags the patterns that match the attacks the
//! repositories know about: typosquatting, dependency confusion against
//! a reserved namespace, a publisher that appears out of nowhere, a
//! release whose metadata changed in a way that does not fit the
//! package's history.
//!
//! Three rules hold this module, and they are the whole design:
//!
//! - **A heuristic is an observation or a hypothesis, never a fact.**
//!   There is no `MalwareFact` in the vocabulary on purpose: a signal
//!   says that a name is close to another name, and that is all it can
//!   say. Classification is fail-closed -- without evidence the state is
//!   `Observation`, and a loud score does not become a claim.
//! - **Action is fail-open.** The default mode is `Detect`: alert, never
//!   hold. `Hold` quarantines ONLY through an explicitly declared
//!   namespace reservation (the policy of HSP-143, the one authorized
//!   rule), and never through name similarity. An unauthorized
//!   similarity can raise an alert; it cannot take a package away.
//! - **Nothing here runs a script.** A package that declares install
//!   hooks is READ, and the declaration becomes a signal. No interpreter
//!   is started, no package is installed, no archive is unpacked --
//!   asserted by construction and policed by a source-scanning test.
//!
//! The namespace policy itself lives in `hodei-catalog::namespaces`
//! (HSP-143). That crate is the authority at resolution time; this
//! module ports only the *judging* semantics it needs to read a name --
//! the Maven groupId with its subgroups, the npm `@scope` or exact name,
//! the PyPI name normalized as PEP 503 -- so that a detector can say
//! "this publication lands inside a reserved namespace" without
//! depending on the catalog crate.

pub mod catalog;
pub mod feedback;
pub mod policy;
pub mod signals;

pub use catalog::{
    Ecosystem, EcosystemCatalog, KnownMetadata, KnownPackage, namespace_of, normalize_name,
};
pub use feedback::{
    FeedbackError, Resolution, ReviewOutcome, ReviewerAuthorization, resolve_false_positive,
};
pub use policy::{Decision, HeuristicPolicy, PolicyMode, Verdict, decide, quarantine_case};
pub use signals::{
    DETECTOR_REVISION, EpistemicStatus, HYPOTHESIS_THRESHOLD, HeuristicSignal, MethodRef,
    Observation, PUBLISHER_AGE_WINDOW_DAYS, PackageRecord, ReasonCode, SignalKind,
    TYPOSQUAT_MAX_DISTANCE, edit_distance_within, evaluate,
};
