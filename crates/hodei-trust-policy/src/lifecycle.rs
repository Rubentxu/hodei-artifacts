//! The lifecycle of a policy set: from a draft to an activated revision.
//!
//! The registry knows which set is active and keeps the history of the
//! activations. This module adds the part that was missing: the **revisions**
//! a set goes through before one of them is the one that decides.
//!
//! A revision is **immutable**: saving a policy computes its digest, and a
//! revision is addressed by `(policy_id, sequence)` and by that digest.
//! Nothing edits a revision — changing a policy means saving a new revision,
//! and so does rolling back: a rollback copies the content of an older
//! revision into a **new** one, which keeps the history strictly append-only
//! and makes every activation name a revision that still exists byte for
//! byte.
//!
//! An activation is a **compare-and-set**: the actor names the head revision
//! it believes is current, and the switch only happens if that belief is
//! true. The check is re-validated inside the storage transaction (see
//! `hodei_trust_policy_storage::lifecycle`), so two activations that race
//! for the same policy produce exactly one winner; the loser is told the
//! revision it expected is not the head and has to read and decide again.
//!
//! Nothing here calls Cedar: this lifecycle decides *which set of
//! requirements is in force*, not *who may do what*. What the strict
//! validation adds (AC1) is that the policy and the facts are checked
//! **before** they can be read as a decision — an unknown dimension, a
//! requirement nothing can satisfy, or context facts that contradict each
//! other are refused rather than tolerated, so a malformed input can never
//! be mistaken for a grant.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::context::{TrustContext, TRUST_CONTEXT_VERSION};
use crate::policy::{RequirementKind, TrustPolicy};
use crate::POLICY_SCHEMA_VERSION;

/// How many revisions of one policy the history keeps.
///
/// The limit exists so that "the history is intact" stays a checkable
/// property for the store: the append-only log of one policy is bounded, and
/// an activation can never be refused because the history grew too large to
/// be written atomically with it.
pub const MAX_REVISIONS: u64 = 1_000;

/// The prefix that marks a revision's reason as a rollback.
pub const ROLLBACK_PREFIX: &str = "rollback:";

/// Why a lifecycle operation was refused.
///
/// The distinction matters: a refusal of the *content* (an invalid policy or
/// context) is not a failure of the *store*, and a lost race is not a
/// refusal at all — it asks the caller to read and decide again.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LifecycleError {
    /// The policy set does not pass the strict validation.
    #[error("the policy set {0} is not valid: {1}")]
    InvalidPolicy(String, String),
    /// The context does not pass the strict validation of its facts.
    #[error("the trust context of {0} is not valid: {1}")]
    InvalidContext(String, String),
    /// The policy set was written against a schema this does not read.
    #[error("the policy set {0} is written against schema {1}, which this does not read")]
    SchemaMismatch(String, String),
    /// There is no such revision.
    #[error("there is no revision {1} of the policy set {0}")]
    RevisionNotFound(String, u64),
    /// The revision's content does not hash to the digest it is stored under.
    #[error("the digest of revision {1} of the policy set {0} does not match its content")]
    CorruptRevision(String, u64),
    /// The revision expected as the head is not the head.
    #[error("the head of {0} is {2}, not the expected revision {1}")]
    StaleHead(String, u64, u64),
    /// A policy has never been activated, so it has no head.
    #[error("the policy set {0} has never been activated")]
    NeverActivated(String),
    /// The history of the policy set is full.
    #[error("the policy set {0} already holds {MAX_REVISIONS} revisions")]
    HistoryFull(String),
    /// The store could not answer.
    #[error("the lifecycle store is not available: {0}")]
    Unavailable(String),
}

impl LifecycleError {
    /// Whether this failure means the content is refused, not the store.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::InvalidPolicy(..)
                | Self::InvalidContext(..)
                | Self::SchemaMismatch(..)
                | Self::CorruptRevision(..)
                | Self::HistoryFull(..)
        )
    }
}

/// How much of the world the caller looked at.
///
/// A decision taken with less than the whole is still recorded, and the
/// shadow report says so (AC2): a policy simulated over three releases when
/// there are forty is a partial reading, and a reader who is not told cannot
/// tell a small diff from an untested one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// The subjects the caller meant to look at.
    pub intended: usize,
    /// The subjects it actually had a context for.
    pub observed: usize,
}

impl Coverage {
    /// The coverage of an observation of `observed` over `intended`.
    pub fn new(intended: usize, observed: usize) -> Self {
        Self { intended, observed }
    }

    /// Whether the whole was seen.
    pub fn is_complete(&self) -> bool {
        self.observed >= self.intended
    }

    /// Whether nothing was seen.
    pub fn is_empty(&self) -> bool {
        self.observed == 0
    }
}

/// The preconditions of an actor over the lifecycle.
///
/// The separation of editor and approver is **configurable** because whether
/// the person who writes a policy may also switch it on is a matter of the
/// organisation. It is off by default, and when it is on the check happens
/// where the decision would otherwise be silent: a revision cannot be saved
/// by an empty actor, and an activation cannot be approved by the person who
/// saved the revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SeparationPolicy {
    /// Whether the one who saved a revision may not activate it.
    pub enabled: bool,
}

impl SeparationPolicy {
    /// The separation the organisation asked for.
    pub fn required() -> Self {
        Self { enabled: true }
    }

    /// Checks an actor that wants to *save* under this separation.
    pub fn check_editor(&self, actor: &str) -> Result<(), LifecycleError> {
        if actor.trim().is_empty() {
            return Err(LifecycleError::InvalidPolicy(
                String::new(),
                "an anonymous actor cannot save a revision".to_string(),
            ));
        }
        Ok(())
    }

    /// Checks an actor that wants to *activate* the given revision.
    pub fn check_approver(
        &self,
        policy_id: &str,
        editor: &str,
        approver: &str,
    ) -> Result<(), LifecycleError> {
        if approver.trim().is_empty() {
            return Err(LifecycleError::InvalidPolicy(
                policy_id.to_string(),
                "an anonymous actor cannot activate a revision".to_string(),
            ));
        }
        if self.enabled && approver == editor {
            return Err(LifecycleError::InvalidPolicy(
                policy_id.to_string(),
                format!(
                    "the separation of editor and approver is required and {approver} saved this revision"
                ),
            ));
        }
        Ok(())
    }
}

/// One immutable revision of a policy set.
///
/// The revision carries the whole `TrustPolicy` plus who saved it, when, its
/// sequence in the history of the set, and the digest of its content. The
/// digest is computed over a canonical envelope of the deciding fields, so a
/// stored row whose content does not hash to its digest is corrupt and is
/// never activated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRevision {
    /// The identifier of the policy set.
    pub policy_id: String,
    /// The position in the history of the set, from 1.
    pub sequence: u64,
    /// The digest of the content, `sha256:<hex>`.
    pub digest: String,
    /// The policy the revision carries.
    pub policy: TrustPolicy,
    /// Who saved it.
    pub saved_by: String,
    /// When it was saved.
    pub saved_at: DateTime<Utc>,
    /// Why this revision exists, in the words of whoever saved it.
    pub reason: String,
}

impl PolicyRevision {
    /// A revision of a policy, with its digest computed over its content.
    pub fn new(
        policy: TrustPolicy,
        sequence: u64,
        saved_by: impl Into<String>,
        saved_at: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Self {
        let saved_by = saved_by.into();
        let policy_id = policy.id.clone();
        let mut revision = Self {
            policy_id,
            sequence,
            digest: String::new(),
            policy,
            saved_by,
            saved_at,
            reason: reason.into(),
        };
        revision.digest = revision.compute_digest();
        revision
    }

    /// The canonical bytes the digest is computed over: the fields that
    /// decide, not the prose that explains.
    fn canonical(&self) -> String {
        let policy = serde_json::to_string(&self.policy).unwrap_or_default();
        format!("hodei-policy-revision:v1\n{}\n{policy}", self.policy_id)
    }

    /// The digest the revision must be stored under.
    fn compute_digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.canonical().as_bytes());
        format!("sha256:{}", hex(hasher.finalize().as_slice()))
    }

    /// Whether the stored content still hashes to the stored digest.
    pub fn digest_holds(&self) -> bool {
        self.digest == self.compute_digest()
    }

    /// Whether this revision rolls an older one back.
    pub fn is_rollback(&self) -> bool {
        self.reason.starts_with(ROLLBACK_PREFIX)
    }
}

/// What happened in one lifecycle operation, for the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleEvent {
    /// Stable identifier of the event.
    pub id: String,
    /// `revision_saved`, `activated`, `rolled_back` or `invalidated`.
    pub kind: String,
    /// The policy set the event belongs to.
    pub policy_id: String,
    /// The revision the event names.
    pub sequence: u64,
    /// The digest of that revision.
    pub digest: String,
    /// The head the operation expected, for an activation.
    pub expected_head: Option<u64>,
    /// The head after the operation.
    pub new_head: u64,
    /// Who did it.
    pub actor: String,
    /// Who saved the revision the approver accepted, for an activation.
    pub editor: String,
    /// Why, in the words of the actor.
    pub reason: String,
    /// When.
    pub at: DateTime<Utc>,
}

/// The pair of actors that carries an operation out.
///
/// The lifecycle has to know who *prepared* a change and who *approved*
/// it — under the separation the two may not be the same person — and
/// naming the pair beats naming its halves in a long argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actors {
    /// The one who saves the revision.
    pub editor: String,
    /// The one who approves the switch.
    pub approver: String,
}

impl Actors {
    /// The pair of one operation.
    pub fn new(editor: impl Into<String>, approver: impl Into<String>) -> Self {
        Self {
            editor: editor.into(),
            approver: approver.into(),
        }
    }
}

/// The head of a policy set: the revision that decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    /// The policy set.
    pub policy_id: String,
    /// The revision that is active.
    pub sequence: u64,
    /// The digest of that revision.
    pub digest: String,
    /// Who switched it on.
    pub activated_by: String,
    /// Who saved the revision that is active.
    pub editor: String,
    /// Why it was activated.
    pub reason: String,
    /// When it was activated.
    pub activated_at: DateTime<Utc>,
}

/// Validates a policy set before it can be saved as a revision.
///
/// The validation is **strict**: a policy that mentions a dimension this
/// version does not know, a requirement that accepts no state (which nothing
/// can ever satisfy), a set with no requirements, a requirement named twice,
/// or a set written against another schema is refused here, before it can
/// become the head of anything. Refusing the content is the point: a
/// half-understood policy that silently grants because one of its readings
/// collapsed is exactly the failure this lifecycle is not allowed to have.
pub fn validate_policy(policy: &TrustPolicy) -> Result<(), LifecycleError> {
    if policy.schema_version != POLICY_SCHEMA_VERSION {
        return Err(LifecycleError::SchemaMismatch(
            policy.id.clone(),
            policy.schema_version.clone(),
        ));
    }
    if policy.id.trim().is_empty() {
        return Err(LifecycleError::InvalidPolicy(
            policy.id.clone(),
            "a policy set needs an identifier".to_string(),
        ));
    }
    if policy.version.trim().is_empty() {
        return Err(LifecycleError::InvalidPolicy(
            policy.id.clone(),
            "a policy set needs a version".to_string(),
        ));
    }
    if policy.requirements.is_empty() {
        return Err(LifecycleError::InvalidPolicy(
            policy.id.clone(),
            "a policy set with no requirements decides nothing".to_string(),
        ));
    }

    let mut names = BTreeSet::new();
    for requirement in &policy.requirements {
        let name = requirement.name();
        if !names.insert(name.clone()) {
            return Err(LifecycleError::InvalidPolicy(
                policy.id.clone(),
                format!("the requirement {name} is named twice"),
            ));
        }
        if let RequirementKind::Dimension { dimension, accepts } = &requirement.kind {
            // An unrecognised dimension name is not a dimension: the
            // requirement would be read as "whatever this turns out to
            // mean", which is a permission with a hole in it.
            if matches!(
                dimension,
                hodei_evidence::domain::capsule::DimensionName::Other(_)
            ) {
                return Err(LifecycleError::InvalidPolicy(
                    policy.id.clone(),
                    format!("the dimension {dimension} is not known to this schema"),
                ));
            }
            if accepts.is_empty() {
                return Err(LifecycleError::InvalidPolicy(
                    policy.id.clone(),
                    format!("the requirement {name} accepts no state"),
                ));
            }
        }
    }
    Ok(())
}

/// Validates the facts of a context before they can be read as evidence for
/// a decision.
///
/// The risk facts have to be consistent with themselves: reachable criticals
/// cannot outnumber known criticals, and a risk that was never established
/// cannot already have accepted VEX statements. A context whose facts
/// contradict each other does not get evaluated — it is refused, so a broken
/// feed cannot turn "we know nothing" into a grant.
pub fn validate_context(context: &TrustContext) -> Result<(), LifecycleError> {
    let subject = context.subject.hrn.clone();
    if context.version != TRUST_CONTEXT_VERSION {
        return Err(LifecycleError::InvalidContext(
            subject,
            format!(
                "the context was built as {}, which this does not read",
                context.version
            ),
        ));
    }
    if subject.trim().is_empty() {
        return Err(LifecycleError::InvalidContext(
            subject,
            "a context needs a subject".to_string(),
        ));
    }
    let mut dimensions = BTreeSet::new();
    for dimension in &context.dimensions {
        if matches!(
            dimension.name,
            hodei_evidence::domain::capsule::DimensionName::Other(_)
        ) {
            return Err(LifecycleError::InvalidContext(
                subject,
                format!(
                    "the dimension {} is not known to this schema",
                    dimension.name
                ),
            ));
        }
        if !dimensions.insert(dimension.name.as_str()) {
            return Err(LifecycleError::InvalidContext(
                subject,
                format!("the dimension {} is stated twice", dimension.name),
            ));
        }
    }
    if context.risk.critical_reachable > context.risk.critical_known {
        return Err(LifecycleError::InvalidContext(
            subject,
            format!(
                "the risk says {} reachable criticals of {} known",
                context.risk.critical_reachable, context.risk.critical_known
            ),
        ));
    }
    if !context.risk.established && context.risk.vex_accepted > 0 {
        return Err(LifecycleError::InvalidContext(
            subject,
            "the risk was never established and already accepted VEX statements".to_string(),
        ));
    }
    Ok(())
}

/// The event of a saved revision.
pub fn saved_event(revision: &PolicyRevision, at: DateTime<Utc>) -> LifecycleEvent {
    LifecycleEvent {
        id: format!("saved:{}:{}", revision.policy_id, revision.sequence),
        kind: "revision_saved".to_string(),
        policy_id: revision.policy_id.clone(),
        sequence: revision.sequence,
        digest: revision.digest.clone(),
        expected_head: None,
        new_head: revision.sequence,
        actor: revision.saved_by.clone(),
        editor: revision.saved_by.clone(),
        reason: revision.reason.clone(),
        at,
    }
}

/// The event of an activation switching the head.
pub fn activation_event(
    revision: &PolicyRevision,
    previous_head: Option<u64>,
    reason: &str,
    approver: &str,
    at: DateTime<Utc>,
) -> LifecycleEvent {
    LifecycleEvent {
        id: format!("activated:{}:{}", revision.policy_id, revision.sequence),
        kind: if revision.is_rollback() {
            "rolled_back".to_string()
        } else {
            "activated".to_string()
        },
        policy_id: revision.policy_id.clone(),
        sequence: revision.sequence,
        digest: revision.digest.clone(),
        expected_head: previous_head,
        new_head: revision.sequence,
        actor: approver.to_string(),
        editor: revision.saved_by.clone(),
        reason: reason.to_string(),
        at,
    }
}

/// The event that says what a rollback invalidated: the content that stops
/// deciding and the content that starts.
pub fn invalidation_event(
    policy_id: &str,
    previous_head: u64,
    previous_digest: &str,
    rolled_back_to: &PolicyRevision,
    actor: &str,
    at: DateTime<Utc>,
) -> LifecycleEvent {
    LifecycleEvent {
        id: format!("invalidated:{policy_id}:{previous_head}"),
        kind: "invalidated".to_string(),
        policy_id: policy_id.to_string(),
        sequence: previous_head,
        digest: previous_digest.to_string(),
        expected_head: Some(previous_head),
        new_head: rolled_back_to.sequence,
        actor: actor.to_string(),
        editor: rolled_back_to.saved_by.clone(),
        reason: format!(
            "the revision {previous_head} stopped deciding, {} decides",
            rolled_back_to.sequence
        ),
        at,
    }
}

/// The mutable state of a lifecycle: the revisions and the head, one pair
/// per policy set.
#[derive(Debug, Default)]
struct Inner {
    /// The revisions by policy set, oldest first.
    revisions: std::collections::BTreeMap<String, Vec<PolicyRevision>>,
    /// The head by policy set.
    heads: std::collections::BTreeMap<String, Head>,
    /// The events, in the order they happened.
    events: Vec<LifecycleEvent>,
}

/// The lifecycle of policy sets: revisions, activation with compare-and-set,
/// and rollback as a new revision.
///
/// It is the in-memory authority over the sequence; the store persists the
/// same sequence with the CAS check inside its transaction. Cloning shares
/// the state through an `Arc`, like the registry does.
#[derive(Debug, Clone, Default)]
pub struct PolicyLifecycle {
    separation: SeparationPolicy,
    inner: Arc<Mutex<Inner>>,
}

impl PolicyLifecycle {
    /// A lifecycle with the given separation of editor and approver.
    pub fn new(separation: SeparationPolicy) -> Self {
        Self {
            separation,
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// A lifecycle that does not require the separation.
    pub fn open() -> Self {
        Self::new(SeparationPolicy::default())
    }

    /// The separation this lifecycle enforces.
    pub fn separation(&self) -> SeparationPolicy {
        self.separation
    }

    /// The sequence the next revision of a policy set will take.
    pub fn next_sequence(&self, policy_id: &str) -> u64 {
        let inner = self.lock();
        inner
            .revisions
            .get(policy_id)
            .map(|revisions| revisions.len() as u64 + 1)
            .unwrap_or(1)
    }

    /// Saves a policy as a new immutable revision, without activating it.
    ///
    /// The content is validated first; a policy that does not hold is
    /// refused and nothing is written.
    pub fn save(
        &self,
        policy: TrustPolicy,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<PolicyRevision, LifecycleError> {
        validate_policy(&policy)?;
        self.separation.check_editor(actor)?;

        let mut inner = self.lock();
        let revision = {
            let revisions = inner.revisions.entry(policy.id.clone()).or_default();
            let sequence = revisions.len() as u64 + 1;
            if sequence > MAX_REVISIONS {
                return Err(LifecycleError::HistoryFull(policy.id.clone()));
            }
            let revision = PolicyRevision::new(policy, sequence, actor, at, reason);
            revisions.push(revision.clone());
            revision
        };
        let event = saved_event(&revision, at);
        inner.events.push(event);
        Ok(revision)
    }

    /// Activates an already-saved revision under a compare-and-set.
    ///
    /// `expected_head` is the revision the caller believes is current; if it
    /// is not, the activation is refused with [`LifecycleError::StaleHead`]
    /// and the head is left untouched. The approver is checked against the
    /// separation with the editor of the revision being activated.
    pub fn activate(
        &self,
        policy_id: &str,
        sequence: u64,
        expected_head: Option<u64>,
        approver: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        let mut inner = self.lock();
        let revision = inner
            .revisions
            .get(policy_id)
            .and_then(|revisions| revisions.iter().find(|r| r.sequence == sequence))
            .cloned()
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), sequence))?;
        if !revision.digest_holds() {
            return Err(LifecycleError::CorruptRevision(
                policy_id.to_string(),
                sequence,
            ));
        }

        let current = inner.heads.get(policy_id).map(|head| head.sequence);
        if current != expected_head {
            return Err(LifecycleError::StaleHead(
                policy_id.to_string(),
                expected_head.unwrap_or(0),
                current.unwrap_or(0),
            ));
        }
        self.separation
            .check_approver(policy_id, &revision.saved_by, approver)?;

        let head = Head {
            policy_id: policy_id.to_string(),
            sequence: revision.sequence,
            digest: revision.digest.clone(),
            activated_by: approver.to_string(),
            editor: revision.saved_by.clone(),
            reason: reason.to_string(),
            activated_at: at,
        };
        inner
            .events
            .push(activation_event(&revision, current, reason, approver, at));
        inner.heads.insert(policy_id.to_string(), head.clone());
        Ok(head)
    }

    /// Saves a new revision and activates it in one step, under the same CAS
    /// the separate calls use.
    pub fn save_and_activate(
        &self,
        policy: TrustPolicy,
        editor: &str,
        approver: &str,
        expected_head: Option<u64>,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        validate_policy(&policy)?;
        let policy_id = policy.id.clone();
        {
            let inner = self.lock();
            let current = inner.heads.get(&policy_id).map(|head| head.sequence);
            if current != expected_head {
                return Err(LifecycleError::StaleHead(
                    policy_id,
                    expected_head.unwrap_or(0),
                    current.unwrap_or(0),
                ));
            }
        }
        let revision = self.save(policy, editor, reason, at)?;
        self.activate(
            &policy_id,
            revision.sequence,
            expected_head,
            approver,
            reason,
            at,
        )
    }

    /// Rolls back to an older revision as a **new** revision.
    ///
    /// The content of `target` is copied into a fresh revision that becomes
    /// the head, and an invalidation event records the revision that stopped
    /// deciding. `actors.editor` prepares the rollback and `actors.approver`
    /// switches it on; under the separation they may not be the same person.
    pub fn rollback(
        &self,
        policy_id: &str,
        target: u64,
        expected_head: Option<u64>,
        actors: &Actors,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        let editor = actors.editor.as_str();
        let approver = actors.approver.as_str();
        let mut inner = self.lock();
        let current = inner
            .heads
            .get(policy_id)
            .map(|head| head.sequence)
            .ok_or_else(|| LifecycleError::NeverActivated(policy_id.to_string()))?;
        if expected_head != Some(current) {
            return Err(LifecycleError::StaleHead(
                policy_id.to_string(),
                expected_head.unwrap_or(0),
                current,
            ));
        }
        let revisions = inner
            .revisions
            .get(policy_id)
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), target))?;
        let source = revisions
            .iter()
            .find(|revision| revision.sequence == target)
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), target))?;
        let previous_digest = revisions
            .iter()
            .find(|revision| revision.sequence == current)
            .map(|revision| revision.digest.clone())
            .unwrap_or_default();

        self.separation.check_editor(editor)?;
        self.separation
            .check_approver(policy_id, editor, approver)?;

        let sequence = revisions.len() as u64 + 1;
        if sequence > MAX_REVISIONS {
            return Err(LifecycleError::HistoryFull(policy_id.to_string()));
        }
        let mut policy = source.policy.clone();
        policy.version = format!("{}-rb{sequence}", policy.version);
        let revision = PolicyRevision::new(
            policy,
            sequence,
            editor,
            at,
            format!("{ROLLBACK_PREFIX}{target} {reason}"),
        );
        let head = Head {
            policy_id: policy_id.to_string(),
            sequence: revision.sequence,
            digest: revision.digest.clone(),
            activated_by: approver.to_string(),
            editor: revision.saved_by.clone(),
            reason: reason.to_string(),
            activated_at: at,
        };

        let revisions = inner.revisions.get_mut(policy_id).expect("present");
        revisions.push(revision.clone());
        inner.events.push(saved_event(&revision, at));
        inner.events.push(activation_event(
            &revision,
            Some(current),
            reason,
            approver,
            at,
        ));
        inner.events.push(invalidation_event(
            policy_id,
            current,
            &previous_digest,
            &revision,
            approver,
            at,
        ));
        inner.heads.insert(policy_id.to_string(), head.clone());
        Ok(head)
    }

    /// The revision of a policy set, if it exists.
    pub fn revision(&self, policy_id: &str, sequence: u64) -> Option<PolicyRevision> {
        let inner = self.lock();
        inner
            .revisions
            .get(policy_id)
            .and_then(|revisions| revisions.iter().find(|r| r.sequence == sequence))
            .cloned()
    }

    /// The whole history of a policy set, oldest first. Never rewritten.
    pub fn history(&self, policy_id: &str) -> Vec<PolicyRevision> {
        let inner = self.lock();
        inner.revisions.get(policy_id).cloned().unwrap_or_default()
    }

    /// The head that decides for a policy set.
    pub fn head(&self, policy_id: &str) -> Option<Head> {
        let inner = self.lock();
        inner.heads.get(policy_id).cloned()
    }

    /// The events, in the order they happened.
    pub fn events(&self) -> Vec<LifecycleEvent> {
        let inner = self.lock();
        inner.events.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("the lifecycle is not poisoned")
    }
}

/// A lowercase hex digest with the algorithm named in front.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::DimensionFact;
    use crate::context::{RiskFacts, SubjectFact, TargetEnvironment};
    use crate::policy::Requirement;
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};

    fn sound_policy() -> TrustPolicy {
        TrustPolicy::new(
            "prod",
            "1.0.0",
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts: vec![DimensionState::Verified],
            })],
        )
    }

    fn context_with(risk: RiskFacts, dimensions: Vec<DimensionFact>) -> TrustContext {
        TrustContext {
            version: TRUST_CONTEXT_VERSION.to_string(),
            subject: SubjectFact {
                hrn: "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web".to_string(),
                digest: None,
            },
            target: TargetEnvironment::Production,
            dimensions,
            capsule_hash: None,
            risk,
            exceptions: Vec::new(),
            evidence_ids: Vec::new(),
            assembled_at: Utc::now(),
        }
    }

    #[test]
    fn a_sound_policy_validates() {
        assert!(validate_policy(&sound_policy()).is_ok());
    }

    #[test]
    fn a_policy_of_another_schema_is_not_read() {
        let mut policy = sound_policy();
        policy.schema_version = "0.9.0".to_string();
        assert_eq!(
            validate_policy(&policy),
            Err(LifecycleError::SchemaMismatch(
                "prod".to_string(),
                "0.9.0".to_string()
            ))
        );
    }

    #[test]
    fn a_requirement_accepting_no_state_is_refused() {
        let policy = TrustPolicy::new(
            "prod",
            "1.0.0",
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts: vec![],
            })],
        );
        assert!(matches!(
            validate_policy(&policy),
            Err(LifecycleError::InvalidPolicy(..))
        ));
    }

    #[test]
    fn an_unknown_dimension_is_refused_and_not_read_as_a_permission() {
        let policy = TrustPolicy::new(
            "prod",
            "1.0.0",
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Other("vibes".to_string()),
                accepts: vec![DimensionState::Verified],
            })],
        );
        let error = validate_policy(&policy).expect_err("the dimension is not known");
        assert!(error.is_refusal());
    }

    #[test]
    fn a_risk_that_does_not_add_up_is_refused() {
        let risk = RiskFacts {
            critical_known: 1,
            critical_reachable: 2,
            established: true,
            ..Default::default()
        };
        let error =
            validate_context(&context_with(risk, Vec::new())).expect_err("the facts contradict");
        assert!(matches!(error, LifecycleError::InvalidContext(..)));
    }

    #[test]
    fn an_unestablished_risk_cannot_have_vex() {
        let risk = RiskFacts {
            established: false,
            vex_accepted: 3,
            ..Default::default()
        };
        assert!(validate_context(&context_with(risk, Vec::new())).is_err());
    }

    #[test]
    fn a_context_stating_a_dimension_twice_is_refused() {
        let facts = vec![
            DimensionFact {
                name: DimensionName::Integrity,
                state: DimensionState::Verified,
            },
            DimensionFact {
                name: DimensionName::Integrity,
                state: DimensionState::Verified,
            },
        ];
        assert!(validate_context(&context_with(Default::default(), facts)).is_err());
    }

    #[test]
    fn a_revision_is_frozen_by_its_digest() {
        let revision = PolicyRevision::new(sound_policy(), 1, "alice", Utc::now(), "the first one");
        assert!(revision.digest_holds());
        assert!(revision.digest.starts_with("sha256:"));

        let mut tampered = revision.clone();
        tampered.policy.version = "9.9.9".to_string();
        assert!(!tampered.digest_holds());
    }

    #[test]
    fn the_approver_may_not_be_the_editor_when_the_separation_is_on() {
        let separation = SeparationPolicy::required();
        assert!(separation.check_approver("prod", "alice", "alice").is_err());
        assert!(separation.check_approver("prod", "alice", "bob").is_ok());
        let open = SeparationPolicy::default();
        assert!(open.check_approver("prod", "alice", "alice").is_ok());
    }

    #[test]
    fn an_anonymous_actor_cannot_save_or_activate() {
        assert!(SeparationPolicy::default().check_editor("").is_err());
        assert!(SeparationPolicy::required()
            .check_approver("prod", "alice", "  ")
            .is_err());
    }

    #[test]
    fn saving_a_bad_policy_writes_nothing() {
        let lifecycle = PolicyLifecycle::open();
        let mut policy = sound_policy();
        policy.requirements.clear();
        let error = lifecycle
            .save(policy, "alice", "bad", Utc::now())
            .expect_err("an empty set decides nothing");
        assert!(error.is_refusal());
        assert!(lifecycle.history("prod").is_empty());
    }

    #[test]
    fn an_activation_that_does_not_expect_the_head_loses() {
        let lifecycle = PolicyLifecycle::open();
        lifecycle
            .save(sound_policy(), "alice", "one", Utc::now())
            .expect("the save holds");
        lifecycle
            .save(sound_policy(), "alice", "two", Utc::now())
            .expect("the save holds");

        // Alice activates revision 1 and the head becomes 1. Bob still
        // believes the head is empty and tries to activate revision 2.
        lifecycle
            .activate("prod", 1, None, "bob", "go", Utc::now())
            .expect("the first activation holds");
        let error = lifecycle
            .activate("prod", 2, None, "carol", "go", Utc::now())
            .expect_err("the head moved under carol");
        assert_eq!(error, LifecycleError::StaleHead("prod".to_string(), 0, 1));
        assert_eq!(lifecycle.head("prod").expect("a head").sequence, 1);
    }

    #[test]
    fn a_rollback_is_a_new_revision_and_invalidates_the_head() {
        let lifecycle = PolicyLifecycle::open();
        lifecycle
            .save(sound_policy(), "alice", "one", Utc::now())
            .expect("the save holds");
        let mut second = sound_policy();
        second.version = "2.0.0".to_string();
        lifecycle
            .save(second, "alice", "two", Utc::now())
            .expect("ok");
        lifecycle
            .activate("prod", 2, None, "bob", "go", Utc::now())
            .expect("active");

        let head = lifecycle
            .rollback(
                "prod",
                1,
                Some(2),
                &Actors::new("bob", "carol"),
                "2 broke staging",
                Utc::now(),
            )
            .expect("the rollback holds");
        assert_eq!(head.sequence, 3);
        assert_eq!(head.editor, "bob");
        assert_eq!(head.activated_by, "carol");
        assert_eq!(
            head.digest,
            lifecycle.revision("prod", 3).expect("r3").digest
        );
        assert_eq!(
            lifecycle.history("prod").len(),
            3,
            "the history grew, never rewrote"
        );
        assert_eq!(
            lifecycle.revision("prod", 1).expect("r1").policy.version,
            "1.0.0",
            "the rolled-back revision is untouched"
        );
        assert!(lifecycle
            .events()
            .iter()
            .any(|event| event.kind == "invalidated" && event.sequence == 2));
    }

    #[test]
    fn the_separation_blocks_self_approval_across_the_lifecycle() {
        let lifecycle = PolicyLifecycle::new(SeparationPolicy::required());
        let error = lifecycle
            .save_and_activate(sound_policy(), "alice", "alice", None, "go", Utc::now())
            .expect_err("alice cannot approve her own revision");
        assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
        assert!(lifecycle.head("prod").is_none());
    }

    #[test]
    fn ten_activations_racing_have_exactly_one_winner() {
        let lifecycle = Arc::new(PolicyLifecycle::open());
        lifecycle
            .save(sound_policy(), "alice", "one", Utc::now())
            .expect("the save holds");

        // Every racer expects the head to be empty and tries to activate
        // revision 1. Only the one that finds the head empty wins; the
        // others see the head already set and lose.
        let winners = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..10 {
                let lifecycle = Arc::clone(&lifecycle);
                let winners = &winners;
                scope.spawn(move || {
                    if lifecycle
                        .activate("prod", 1, None, "bot", "go", Utc::now())
                        .is_ok()
                    {
                        winners.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                });
            }
        });
        assert_eq!(winners.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(lifecycle.head("prod").expect("a head").sequence, 1);
    }
}
