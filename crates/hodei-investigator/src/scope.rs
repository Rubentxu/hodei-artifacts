//! The retrieval scope: what can be looked at and what cannot.
//!
//! An investigation is carried out over a subgraph, and the subgraph is requested
//! **explicitly**: the asker (verified), the subject and the limits.
//! A query that goes beyond what was requested is not a query: it is a leak,
//! and a leak from one account to another is the worst failure this
//! system can have.
//!
//! That is why the scope is checked **twice**: once when retrieving
//! (the retriever authorizes BEFORE reading) and once when used (the one that
//! investigates checks what it was given). The second check is the
//! one that matters: a retriever that makes a mistake cannot leak anything.
//!
//! And accounts are NOT compared with text: the **canonical HRN
//! components** are compared. An ad hoc `contains` or `split` is a
//! check that a similar HRN fools; `account_id()` is not.

use async_trait::async_trait;
use hodei_evidence::domain::graph::NodeKind;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::read_set::ReadSet;
use crate::redact::redact;

/// How much a text is trusted.
///
/// An evidence text is **data**, never instruction. An SBOM description
/// is written by whoever publishes the package, and a README by
/// whoever wants: both are attacker text as soon as the attacker
/// is the one publishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// The text is evidence: it is read, cited, not obeyed.
    Untrusted,
    /// The text was written by the system: that is indeed instruction.
    Trusted,
}

/// The budget of a retrieval.
///
/// An investigation is a resource, and the context given to the model
/// has a cost: how many pieces of evidence are loaded, how many relations
/// enter and how many bytes of content are downloaded. The limits are
/// **declared**, and what does not fit is OMITTED, not truncated: a
/// whole blob is not downloaded to cut it afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBudget {
    /// How many pieces of evidence are loaded, at most.
    pub max_objects: usize,
    /// How many relation facts enter, at most.
    pub max_relations: usize,
    /// How many bytes of content are downloaded, in total.
    pub max_bytes: u64,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        Self {
            max_objects: 100,
            max_relations: 400,
            max_bytes: 256 * 1024,
        }
    }
}

/// The scope of a retrieval.
///
/// The actor is the **verified** identity that requests the investigation, and
/// the account is NOT stated: it is taken from the subject by its canonical component.
/// An actor of account A and a subject of account B do not form a scope: both
/// checks reject it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalScope {
    /// Who asks, verified.
    pub actor: Hrn,
    /// The subject to start from.
    pub subject: Hrn,
    /// How many nodes the retrieval may touch.
    pub max_nodes: usize,
    /// How deep it may go.
    pub max_depth: usize,
    /// How much the retrieval may spend.
    #[serde(default)]
    pub budget: RetrievalBudget,
}

impl RetrievalScope {
    /// A scope with the default limits.
    pub fn new(actor: Hrn, subject: Hrn) -> Self {
        Self {
            actor,
            subject,
            max_nodes: 200,
            max_depth: 4,
            budget: RetrievalBudget::default(),
        }
    }

    /// The account of the investigation, by the canonical component of the
    /// subject. It is never stated as free text.
    pub fn the_account(&self) -> &str {
        self.subject.account_id()
    }

    /// Whether the scope crosses accounts: an actor and a subject of different
    /// accounts do not form a scope.
    pub fn crosses_accounts(&self) -> bool {
        self.actor.account_id() != self.subject.account_id()
    }
}

/// The state of a retrieved fact.
///
/// The distinctions ARE the contract: absent content (Missing), a
/// store that did not answer (Unavailable), a text that could not be
/// decoded (Undecodable) and a fact that the budget left out
/// (OmittedByBudget) are four different things, and none of them is
/// disguised as invented text or as a silent absence of facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// The content was read and is delivered.
    Loaded,
    /// The evidence exists and its content is not there.
    Missing,
    /// The store could not answer.
    Unavailable,
    /// The content exists and could not be decoded as text.
    Undecodable,
    /// The budget left it out BEFORE it was read.
    OmittedByBudget,
}

impl Default for FactStatus {
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self::Loaded
    }
}

/// How much of what exists was delivered.
///
/// A partial retrieval is DECLARED: a known but incomplete scope
/// cannot be presented as an exhaustive answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// Everything that exists was delivered.
    #[default]
    Complete,
    /// A part was delivered, and it says how much was left out.
    Partial {
        /// How many facts were left out by the budget.
        omitted: usize,
    },
}

/// A retrieved fact, with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedFact {
    /// The identifier of the evidence.
    ///
    /// It is what is cited, and a citation that does not resolve is not valid.
    pub evidence_id: String,
    /// The text. It is DATA, never instruction.
    pub text: String,
    /// The graph path by which the fact was reached.
    pub path: Vec<Hrn>,
    /// What kind of node the path's subject is.
    pub kind: NodeKind,
    /// How much the text is trusted.
    pub trust: TrustLevel,
    /// The state of the fact. By default, loaded: facts that came
    /// without a state are facts that were read.
    #[serde(default)]
    pub status: FactStatus,
}

impl ScopedFact {
    /// An evidence fact, which is data and not instruction. The text is
    /// REDACTED right here: what goes out to the model carries no
    /// secrets.
    pub fn untrusted(
        evidence_id: impl Into<String>,
        text: impl Into<String>,
        path: Vec<Hrn>,
    ) -> Self {
        Self {
            evidence_id: evidence_id.into(),
            text: redact(&text.into()),
            path,
            kind: NodeKind::Evidence,
            trust: TrustLevel::Untrusted,
            status: FactStatus::Loaded,
        }
    }
}

/// What a retrieval delivers: the facts, HOW MUCH of what exists, and
/// the manifest of what was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedContext {
    /// The facts, in the order of their stable key.
    pub facts: Vec<ScopedFact>,
    /// Whether the retrieval was complete or something was omitted.
    pub coverage: Coverage,
    /// The manifest of the reads: identifiers, digests and
    /// generations, without raw content.
    pub read_set: ReadSet,
}

/// Why the retrieval could not be carried out.
///
/// An authorization rejection is EXPLICIT: never an empty answer
/// that looks like zero evidence, and never a text that has to be guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalError {
    /// The asker cannot read the resource. The door is closed.
    NotAuthorized {
        /// Who was asking.
        actor: String,
        /// The resource that cannot be read.
        resource: String,
    },
    /// The world changed while being read: the generation of a source at the
    /// start of the read is no longer the one at the end. There is no coherent
    /// snapshot to present, and an obsolete read is declared
    /// obsolete.
    Stale {
        source: String,
        expected: u64,
        found: u64,
    },
    /// The store could not serve. Denying is closing: if the permission
    /// could not be checked, it is not read.
    Unavailable(String),
}

impl std::fmt::Display for RetrievalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized { actor, resource } => {
                write!(formatter, "{actor} is not allowed to read {resource}")
            }
            Self::Stale {
                source,
                expected,
                found,
            } => write!(
                formatter,
                "the world changed while reading: {source} was at generation {expected} and is at {found}"
            ),
            Self::Unavailable(reason) => {
                write!(formatter, "the retrieval is unavailable: {reason}")
            }
        }
    }
}

/// The read permission for the evidence.
///
/// The shared Authorizer arrives through this port: every resource that is
/// read is authorized BEFORE being read, and a denial or an error is a
/// closed door, never a silent read. AI is not an authority:
/// this port is implemented by the deterministic policy engine.
#[async_trait]
pub trait EvidenceReadAuthorizer: Send + Sync {
    /// Ok if the actor can read the resource; Err with the reason if not.
    async fn can_read(&self, actor: &Hrn, resource: &Hrn) -> Result<(), String>;
}

/// Why a fact does not fit in the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeViolation {
    /// The fact's subject is not the question's subject.
    AnotherSubject,
    /// The path is deeper than allowed.
    TooDeep(usize),
    /// There are more facts than fit.
    TooMany(usize),
    /// The path goes through a node of another account.
    AnotherAccount,
}

impl std::fmt::Display for ScopeViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnotherSubject => {
                write!(formatter, "the fact is about another subject")
            }
            Self::TooDeep(depth) => {
                write!(
                    formatter,
                    "the path is {depth} hops deep and the scope allows fewer"
                )
            }
            Self::TooMany(count) => {
                write!(
                    formatter,
                    "there are {count} facts and the scope allows fewer"
                )
            }
            Self::AnotherAccount => {
                write!(
                    formatter,
                    "the path leaves the account of the investigation"
                )
            }
        }
    }
}

/// Checks that the facts fit in the scope.
///
/// This is the check that **does not trust** whoever retrieved: the
/// service does it over what it was given, and that is why a retriever that
/// makes a mistake cannot let anything in. Accounts are compared by the
/// canonical components of the HRN, never by text.
pub fn check_the_scope(scope: &RetrievalScope, facts: &[ScopedFact]) -> Result<(), ScopeViolation> {
    // A scope that already crosses accounts is not a scope: nothing it
    // brings belongs to the asker's account.
    if scope.crosses_accounts() {
        return Err(ScopeViolation::AnotherAccount);
    }

    if facts.len() > scope.max_nodes {
        return Err(ScopeViolation::TooMany(facts.len()));
    }

    for fact in facts {
        if fact.path.is_empty() {
            return Err(ScopeViolation::AnotherSubject);
        }
        if fact.path.len() > scope.max_depth + 1 {
            return Err(ScopeViolation::TooDeep(fact.path.len() - 1));
        }
        // The path must start at the question's subject and
        // cannot leave the account: canonical component against
        // canonical component.
        if fact.path.first() != Some(&scope.subject) {
            return Err(ScopeViolation::AnotherSubject);
        }
        for node in &fact.path {
            if node.account_id() != scope.subject.account_id() {
                return Err(ScopeViolation::AnotherAccount);
            }
        }
    }

    Ok(())
}
