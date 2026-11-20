//! The shadow evaluation: what a candidate would decide, read side by side
//! with what the active revision decides — **without executing anything**.
//!
//! The simulation in [`crate::simulation`] answers "which releases would
//! change". This module answers the sharper question the lifecycle needs
//! before an activation: the **diff by outcome with its reasons**, over a
//! snapshot that says how much of the world was actually looked at.
//!
//! Three properties hold by construction (AC2):
//!
//! - **No effects.** The function receives the active policy and the
//!   contexts as data. It has no registry, no store, and no way to promote
//!   anything; the report says so in a field, `effects_executed`, which is
//!   always false.
//! - **Reasons travel with the diff.** Every entry carries the reason codes
//!   of both readings, so an `ALLOW -> DENY` flip names the requirement that
//!   turned, not just its direction.
//! - **The snapshot says what it covers.** When the caller looked at fewer
//!   subjects than exist, the report is marked `snapshot_complete: false`
//!   with the coverage counts. A small diff over three releases when there
//!   are forty is not a clean candidate; it is an untested one, and the
//!   reader can tell the two apart.
//!
//! Fail-closed ties the shadow to the lifecycle (AC1): a context whose
//! facts do not validate is never read as a grant — its entries come out
//! `INDETERMINATE` with the validation reason, and an invalid candidate is
//! refused outright. A shadow that quietly tolerated broken evidence would
//! produce exactly the diff that flatters the candidate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::decision::{evaluate, Outcome};
use crate::lifecycle::{validate_context, validate_policy, Coverage, LifecycleError};
use crate::policy::TrustPolicy;
use crate::reasons::ReasonCode;

/// What one subject looks like under both policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowEntry {
    /// The subject, as the snapshot names it.
    pub subject: String,
    /// What the active revision says, or `Indeterminate` if there is none.
    pub current: Outcome,
    /// What the candidate would say.
    pub candidate: Outcome,
    /// The reasons of the current reading.
    pub current_reasons: Vec<ReasonCode>,
    /// The reasons of the candidate reading.
    pub candidate_reasons: Vec<ReasonCode>,
    /// Whether the two disagree.
    pub changed: bool,
    /// The validation failure of the context, if it did not validate.
    ///
    /// A context that does not validate is not evaluated at all: both
    /// readings are `Indeterminate` and this field says why. The alternative
    /// — evaluating broken facts — is how a permission is minted out of a
    /// mistake in a feed.
    pub validation_error: Option<String>,
}

impl ShadowEntry {
    /// Whether the candidate would let through what the active policy
    /// refused. This is the direction a reviewer has to look at first.
    pub fn loosens(&self) -> bool {
        self.current == Outcome::Deny && self.candidate == Outcome::Allow
    }

    /// Whether the candidate would refuse what the active policy allowed.
    pub fn tightens(&self) -> bool {
        self.current == Outcome::Allow && self.candidate == Outcome::Deny
    }
}

/// The report of a shadow evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowReport {
    /// The identifier of the candidate set.
    pub candidate_policy_id: String,
    /// The version of the candidate set.
    pub candidate_policy_version: String,
    /// The digest of the candidate, when the lifecycle has one.
    pub candidate_digest: Option<String>,
    /// The version of the revision that is active, if any.
    pub active_policy_version: Option<String>,
    /// One entry per subject in the snapshot.
    pub entries: Vec<ShadowEntry>,
    /// The subjects the candidate would let through that were refused.
    pub loosened: Vec<String>,
    /// The subjects the candidate would refuse that were let through.
    pub tightened: Vec<String>,
    /// The subjects either side reads as `Indeterminate`.
    pub indeterminate: Vec<String>,
    /// How much of the world the snapshot covers.
    pub coverage: Coverage,
    /// Whether the snapshot covers everything (AC2: false is *shown*).
    pub snapshot_complete: bool,
    /// Always false, and written down so that a reader of the report does
    /// not have to take it on faith: the shadow executed no effect.
    pub effects_executed: bool,
    /// When the shadow was read.
    pub taken_at: DateTime<Utc>,
}

impl ShadowReport {
    /// The number of subjects whose result changes.
    pub fn changed_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.changed).count()
    }

    /// The entries whose result changes.
    pub fn changed(&self) -> impl Iterator<Item = &ShadowEntry> {
        self.entries.iter().filter(|entry| entry.changed)
    }
}

/// Reads the candidate next to the active revision over a snapshot.
///
/// The active policy is `None` for a set that has never been activated, and
/// every reading against it is `Indeterminate`: there is no permission to
/// compare against. A candidate that does not validate is refused with an
/// error, not reported.
///
/// `coverage` is the caller's honesty about the snapshot: how many subjects
/// exist (`intended`) against how many it supplied (`observed`). Partial
/// coverage produces `snapshot_complete: false`, which the activation
/// review has to see.
pub fn shadow_evaluate(
    active: Option<&TrustPolicy>,
    candidate: &TrustPolicy,
    contexts: &[(String, TrustContext)],
    coverage: Coverage,
    taken_at: DateTime<Utc>,
) -> Result<ShadowReport, LifecycleError> {
    validate_policy(candidate)?;

    let mut entries = Vec::with_capacity(contexts.len());
    let mut loosened = Vec::new();
    let mut tightened = Vec::new();
    let mut indeterminate = Vec::new();

    for (subject, context) in contexts {
        let validation = match validate_context(context) {
            Ok(()) => None,
            Err(error) => Some(error.to_string()),
        };

        let (current, current_reasons, candidate_outcome, candidate_reasons) = match &validation {
            // The facts do not hold together. Nothing is read from them:
            // both sides are indeterminate, and the absence of a decision
            // is not a permission.
            Some(_) => (
                Outcome::Indeterminate,
                vec![ReasonCode::ContextUnavailable],
                Outcome::Indeterminate,
                vec![ReasonCode::ContextUnavailable],
            ),
            None => {
                let current: (Outcome, Vec<ReasonCode>) = match active {
                    Some(policy) => {
                        let decision = evaluate(context, policy);
                        (decision.outcome, decision.reasons)
                    }
                    // There is no revision to compare against. The absence
                    // is not a permission the candidate may flip: it is
                    // indeterminate.
                    None => (Outcome::Indeterminate, vec![ReasonCode::PolicyUnavailable]),
                };
                let candidate_decision = evaluate(context, candidate);
                (
                    current.0,
                    current.1,
                    candidate_decision.outcome,
                    candidate_decision.reasons,
                )
            }
        };

        let changed = current != candidate_outcome;
        if changed && candidate_outcome == Outcome::Allow {
            loosened.push(subject.clone());
        }
        if changed && candidate_outcome == Outcome::Deny {
            tightened.push(subject.clone());
        }
        if current == Outcome::Indeterminate || candidate_outcome == Outcome::Indeterminate {
            indeterminate.push(subject.clone());
        }

        entries.push(ShadowEntry {
            subject: subject.clone(),
            current,
            candidate: candidate_outcome,
            current_reasons,
            candidate_reasons,
            changed,
            validation_error: validation,
        });
    }

    let snapshot_complete = coverage.is_complete() && coverage.intended > 0;

    Ok(ShadowReport {
        candidate_policy_id: candidate.id.clone(),
        candidate_policy_version: candidate.version.clone(),
        candidate_digest: None,
        active_policy_version: active.map(|policy| policy.version.clone()),
        entries,
        loosened,
        tightened,
        indeterminate,
        coverage,
        snapshot_complete,
        // The function has no way to touch anything, and the field says so.
        effects_executed: false,
        taken_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{DimensionFact, RiskFacts, SubjectFact, TargetEnvironment};
    use crate::policy::{Requirement, RequirementKind};
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};

    fn policy_accepting(version: &str, accepts: Vec<DimensionState>) -> TrustPolicy {
        TrustPolicy::new(
            "prod",
            version,
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts,
            })],
        )
    }

    fn subject(name: &str) -> String {
        format!("hrn:hodei:artifacts:eu-west-1:acme:Artifact/{name}")
    }

    fn context(state: DimensionState) -> TrustContext {
        TrustContext {
            version: crate::context::TRUST_CONTEXT_VERSION.to_string(),
            subject: SubjectFact {
                hrn: subject("web"),
                digest: None,
            },
            target: TargetEnvironment::Production,
            dimensions: vec![DimensionFact {
                name: DimensionName::Integrity,
                state,
            }],
            capsule_hash: None,
            risk: RiskFacts::default(),
            exceptions: Vec::new(),
            evidence_ids: Vec::new(),
            assembled_at: Utc::now(),
        }
    }

    #[test]
    fn the_shadow_never_executes_effects() {
        let report = shadow_evaluate(
            Some(&policy_accepting("1.0.0", vec![DimensionState::Verified])),
            &policy_accepting("2.0.0", vec![DimensionState::Verified]),
            &[(subject("web"), context(DimensionState::Verified))],
            Coverage::new(1, 1),
            Utc::now(),
        )
        .expect("the shadow reads");
        assert!(!report.effects_executed);
        assert!(report.snapshot_complete);
    }

    #[test]
    fn a_diff_carries_its_reasons() {
        // The active policy only accepts Verified; the candidate also
        // accepts Unverified. An unverified artefact flips Deny -> Allow,
        // and the report says so with the reason that turned.
        let report = shadow_evaluate(
            Some(&policy_accepting("1.0.0", vec![DimensionState::Verified])),
            &policy_accepting(
                "2.0.0",
                vec![DimensionState::Verified, DimensionState::Unverified],
            ),
            &[(subject("web"), context(DimensionState::Unverified))],
            Coverage::new(1, 1),
            Utc::now(),
        )
        .expect("the shadow reads");

        let entry = report.entries.first().expect("one entry");
        assert_eq!(entry.current, Outcome::Deny);
        assert_eq!(entry.candidate, Outcome::Allow);
        assert!(entry.loosens());
        assert_eq!(report.loosened, vec![subject("web")]);
        assert!(entry
            .current_reasons
            .contains(&ReasonCode::TrustSignatureUnverified));
        assert!(entry.candidate_reasons.is_empty());
    }

    #[test]
    fn a_partial_snapshot_is_marked_incomplete() {
        let report = shadow_evaluate(
            Some(&policy_accepting("1.0.0", vec![DimensionState::Verified])),
            &policy_accepting("2.0.0", vec![DimensionState::Verified]),
            &[(subject("web"), context(DimensionState::Verified))],
            Coverage::new(40, 1),
            Utc::now(),
        )
        .expect("the shadow reads");
        assert!(!report.snapshot_complete, "three of forty is not all");
        assert_eq!(report.coverage.intended, 40);
        assert_eq!(report.coverage.observed, 1);
    }

    #[test]
    fn broken_facts_come_out_indeterminate_and_never_allow() {
        // The risk facts say two reachable criticals of one known: the
        // context does not validate. Neither side may read it as a grant.
        let mut broken = context(DimensionState::Verified);
        broken.risk = RiskFacts {
            critical_known: 1,
            critical_reachable: 2,
            established: true,
            ..Default::default()
        };
        let report = shadow_evaluate(
            Some(&policy_accepting("1.0.0", vec![DimensionState::Verified])),
            &policy_accepting("2.0.0", vec![DimensionState::Verified]),
            &[("web".to_string(), broken)],
            Coverage::new(1, 1),
            Utc::now(),
        )
        .expect("the shadow reads");

        let entry = report.entries.first().expect("one entry");
        assert_eq!(entry.current, Outcome::Indeterminate);
        assert_eq!(entry.candidate, Outcome::Indeterminate);
        assert!(!entry.loosens());
        assert_eq!(
            entry.candidate_reasons,
            vec![ReasonCode::ContextUnavailable]
        );
        assert!(entry.validation_error.is_some());
        assert_eq!(report.indeterminate, vec!["web".to_string()]);
    }

    #[test]
    fn an_invalid_candidate_is_refused_not_reported() {
        let error = shadow_evaluate(
            None,
            &policy_accepting("2.0.0", Vec::new()),
            &[],
            Coverage::new(1, 1),
            Utc::now(),
        )
        .expect_err("a requirement accepting nothing is refused");
        assert!(matches!(error, LifecycleError::InvalidPolicy(..)));
    }

    #[test]
    fn with_no_active_revision_everything_is_indeterminate() {
        let report = shadow_evaluate(
            None,
            &policy_accepting("1.0.0", vec![DimensionState::Verified]),
            &[(subject("web"), context(DimensionState::Verified))],
            Coverage::new(1, 1),
            Utc::now(),
        )
        .expect("the shadow reads");
        assert_eq!(report.active_policy_version, None);
        let entry = report.entries.first().expect("one entry");
        assert_eq!(entry.current, Outcome::Indeterminate);
        assert_eq!(entry.current_reasons, vec![ReasonCode::PolicyUnavailable]);
    }
}
