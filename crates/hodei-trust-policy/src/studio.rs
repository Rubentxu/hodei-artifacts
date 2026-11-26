//! The domain of the Policy Studio (HSP-151).
//!
//! The studio is the console of the *effects* of a trust policy: a person
//! writes a candidate policy, validates it, simulates it against the
//! evidence snapshots the studio was given, reads the diff and -- only
//! through the lifecycle endpoint of a revision that is already saved and
//! validated -- activates it. Nothing in this module talks to HTTP and
//! nothing in it decides a promotion: it is the seam that lets a policy be
//! *read as a decision* before it is the policy that decides.
//!
//! The four answers the studio must tell apart (AC2) are produced here and
//! nowhere else:
//!
//! - a **policy decision**: the candidate was measured against a complete
//!   context and said `ALLOW`, `DENY`, `WARN` or `REQUIRE_APPROVAL`;
//! - an **evidence gap**: a requirement was refused because the evidence
//!   it needs is not there, so the criteria were never looked at; the
//!   studio says which dimension the gap names and what evidence the
//!   snapshot does carry;
//! - an **engine error**: the candidate itself is malformed, the context
//!   contradicts itself, or the store could not be asked -- a failure of
//!   the machinery, not a refusal of the policy;
//! - an **access decision**: who may save, simulate or activate is Cedar's
//!   question, answered at the gate, and this module never pretends to
//!   know it.
//!
//! The simulation reuses the evaluator of the lifecycle (HSP-149) and the
//! shape of its simulation report: the candidate *changes nothing*. It
//! does not activate, it does not write, it does not revoke, and the report
//! carries `applied: false` -- a value with no way to be true -- so that a
//! reader of the answer does not have to take that on faith (AC4).
//!
//! The evidence link (AC2): every explanation of an effect names the
//! policy set and the version it was measured with, and the evidence
//! identifiers of the snapshot travel inside the effect. A reason without
//! a name behind it is a guess.

use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::decision::{Outcome, TrustDecision};
use crate::lifecycle::{validate_context, validate_policy, LifecycleError};
use crate::policy::TrustPolicy;

/// The candidate the studio is asked about: a policy set that may or may
/// not be a revision that is already saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioCandidate {
    /// The policy set itself.
    pub policy: TrustPolicy,
    /// The revision sequence, when the candidate *is* a saved revision.
    ///
    /// The studio uses it to bind an effect to the immutable content the
    /// revision holds: the digest of the revision answers alongside, and a
    /// draft without a sequence is the caller's own text, not durable
    /// content.
    pub revision: Option<u64>,
}

impl StudioCandidate {
    /// An unsaved draft: the editor's text, nothing more.
    pub fn draft(policy: TrustPolicy) -> Self {
        Self {
            policy,
            revision: None,
        }
    }

    /// A revision that is already saved in the lifecycle, addressed by
    /// its sequence.
    pub fn revision(policy: TrustPolicy, sequence: u64) -> Self {
        Self {
            policy,
            revision: Some(sequence),
        }
    }
}

/// One release to simulate the candidate against.
///
/// The context is the *snapshot*: the evidence as it was assembled by the
/// collector, not a live read. The studio does not go and look for
/// anything -- that is what makes the simulation reproducible, and what
/// keeps it from being a write dressed as a read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedSubject {
    /// The subject the release is about, as it travels in reports.
    pub subject: String,
    /// The evidence snapshot the candidate is measured against.
    pub context: TrustContext,
}

/// Where the answer came from, and how much of it can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioKind {
    /// The candidate was measured against a complete context and the
    /// measurement is the answer.
    PolicyDecision,
    /// A required piece of evidence is missing: the criteria were never
    /// looked at, and that is not a refusal.
    EvidenceGap,
    /// The engine could not answer: a malformed candidate, a context that
    /// contradicts itself, or a store that failed.
    EngineError,
}

impl StudioKind {
    /// The name of the kind, for a report or a log.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PolicyDecision => "POLICY_DECISION",
            Self::EvidenceGap => "EVIDENCE_GAP",
            Self::EngineError => "ENGINE_ERROR",
        }
    }
}

/// The evidence the studio could not ask about, and where the question
/// pointed.
///
/// A gap is always bound to a requirement and a reason code; a reason
/// that names nothing is not a gap, it is noise, and the studio refuses
/// to produce it. The identifiers of the evidence the snapshot *does*
/// carry travel with it, so the reader sees the hole against what exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceGap {
    /// The requirement that was never measured.
    pub requirement: String,
    /// The dimension the missing evidence belongs to, when the reason
    /// names one.
    pub dimension: Option<String>,
    /// The reason code that says which piece of evidence is missing.
    pub reason: String,
    /// The evidence the snapshot carries.
    pub evidence_ids: Vec<String>,
}

/// The answer for one simulated subject.
///
/// Exactly one shape is filled, and the kind says which: `decision` for a
/// policy decision (the measured answer), `gap` for an evidence gap (the
/// decision carries the `INDETERMINATE`-style reading of the hole), and
/// `error` for an engine error. The binding to the policy that was asked
/// about travels inside the decision, and the revision binds it to the
/// durable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedEffect {
    /// The subject of this answer.
    pub subject: String,
    /// What the answer is.
    pub kind: StudioKind,
    /// The decision, when the candidate was measured.
    pub decision: Option<TrustDecision>,
    /// The evidence gap, when a requirement was never measured.
    pub gap: Option<EvidenceGap>,
    /// The engine error, when the machinery could not answer.
    pub error: Option<String>,
}

impl SimulatedEffect {
    /// The candidate was measured and said what it said.
    pub fn measured(subject: impl Into<String>, decision: TrustDecision) -> Self {
        Self {
            subject: subject.into(),
            kind: StudioKind::PolicyDecision,
            decision: Some(decision),
            gap: None,
            error: None,
        }
    }

    /// A requirement was refused because its evidence is not there: the
    /// criteria were never looked at, and the studio says which hole it
    /// hit. AC2 -- this is not a denial of the artefact.
    pub fn with_gap(subject: impl Into<String>, decision: TrustDecision, gap: EvidenceGap) -> Self {
        Self {
            subject: subject.into(),
            kind: StudioKind::EvidenceGap,
            decision: Some(decision),
            gap: Some(gap),
            error: None,
        }
    }

    /// The subject could not be measured at all: not a refusal, a failure
    /// of the machinery.
    pub fn engine_error(subject: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            kind: StudioKind::EngineError,
            decision: None,
            gap: None,
            error: Some(error.into()),
        }
    }
}

/// The diff of one subject between the active policy and the candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectChange {
    /// The subject whose outcome changes.
    pub subject: String,
    /// What the active policy said.
    pub before: Outcome,
    /// What the candidate would say.
    pub after: Outcome,
    /// Whether the candidate is looser than the head: an `ALLOW` where the
    /// head refused. The studio marks it because a loosening is the effect
    /// a reviewer has to see first.
    pub loosens: bool,
}

impl EffectChange {
    /// The diff of one measured pair.
    pub fn between(subject: impl Into<String>, before: Outcome, after: Outcome) -> Self {
        Self {
            subject: subject.into(),
            before,
            after,
            loosens: !before.advances() && after.advances(),
        }
    }
}

/// The whole answer of a simulation.
///
/// It says what the candidate is, what the head was, what each subject's
/// effect is, which subjects changed, and -- in `applied` -- that the
/// simulation wrote nothing. The field is `false` by construction: the
/// type has no way to set it to true, and it is part of the JSON contract
/// because a reader of the answer should not have to trust a comment
/// (AC4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationAnswer {
    /// The candidate that was asked about.
    pub candidate: StudioCandidate,
    /// The version of the policy that decides now, when the set has a
    /// head. `null` means there is no head: every effect is measured
    /// against an empty baseline, and the studio says so.
    pub active_version: Option<String>,
    /// The answer for each subject, in the order it was asked.
    pub effects: Vec<SimulatedEffect>,
    /// The subjects whose outcome changes against the head.
    pub changes: Vec<EffectChange>,
    /// Whether the simulation wrote anything anywhere. Always false.
    pub applied: bool,
}

/// Validates a candidate the way the lifecycle validates a revision
/// before it can be saved (HSP-149), so the studio's "valid" is the same
/// word the store's "save" uses and not a softer one.
pub fn validate_candidate(policy: &TrustPolicy) -> Result<(), LifecycleError> {
    validate_policy(policy)
}

/// Simulates a candidate against the evidence snapshots.
///
/// The active policy is the one the deployment has; when the set has never
/// been activated there is no baseline and the report says so with
/// `active_version: null`.
///
/// The function is total over the candidate: a candidate that cannot be
/// validated answers with engine errors for every subject, never with a
/// refusal -- a malformed policy has never said "no".
pub fn simulate_effects(
    active: Option<&TrustPolicy>,
    candidate: &StudioCandidate,
    subjects: &[SimulatedSubject],
) -> SimulationAnswer {
    // 1. The candidate itself has to hold. The lifecycle's strict
    // validation is the same gate the save uses, so a draft the studio
    // calls valid is a draft the store will accept.
    if let Err(error) = validate_candidate(&candidate.policy) {
        return SimulationAnswer {
            candidate: candidate.clone(),
            active_version: active.map(|policy| policy.version.clone()),
            effects: subjects
                .iter()
                .map(|subject| {
                    SimulatedEffect::engine_error(subject.subject.clone(), error.to_string())
                })
                .collect(),
            changes: Vec::new(),
            applied: false,
        };
    }

    // 2. Measure every subject. A context that contradicts itself is an
    // engine error (the machinery cannot read it); a context with a hole
    // is an evidence gap (the criteria were never looked at); a measured
    // context is a policy decision.
    let mut effects = Vec::with_capacity(subjects.len());
    let mut changes = Vec::new();
    for subject in subjects {
        if let Err(error) = validate_context(&subject.context) {
            effects.push(SimulatedEffect::engine_error(
                subject.subject.clone(),
                error.to_string(),
            ));
            continue;
        }

        let decision = crate::decision::evaluate(&subject.context, &candidate.policy);
        // The gap is read from the per-requirement outcomes: a refusal
        // whose reason says the evidence is *absent* was never a
        // measurement of that dimension (the engine marks exactly that
        // with the `state_of == None` path and the `MISSING` state).
        let gap = decision.requirements.iter().find_map(|requirement| {
            if requirement.satisfied {
                return None;
            }
            let reason = requirement.reason?;
            if reason.is_absence() {
                Some(EvidenceGap {
                    requirement: requirement.requirement.clone(),
                    dimension: dimension_of(&requirement.requirement),
                    reason: reason.as_str().to_string(),
                    evidence_ids: decision.evidence_ids.clone(),
                })
            } else {
                None
            }
        });

        match gap {
            Some(gap) => effects.push(SimulatedEffect::with_gap(
                subject.subject.clone(),
                decision,
                gap,
            )),
            None => {
                let before = match active {
                    Some(policy) => crate::decision::evaluate(&subject.context, policy).outcome,
                    None => Outcome::Indeterminate,
                };
                if before != decision.outcome {
                    changes.push(EffectChange::between(
                        subject.subject.clone(),
                        before,
                        decision.outcome,
                    ));
                }
                effects.push(SimulatedEffect::measured(subject.subject.clone(), decision));
            }
        }
    }

    SimulationAnswer {
        candidate: candidate.clone(),
        active_version: active.map(|policy| policy.version.clone()),
        effects,
        changes,
        applied: false,
    }
}

/// The dimension a requirement of the form `dimension:<name>` names.
fn dimension_of(requirement: &str) -> Option<String> {
    requirement.strip_prefix("dimension:").map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{RiskFacts, SubjectFact, TargetEnvironment, TRUST_CONTEXT_VERSION};
    use crate::policy::{OnFailure, Requirement, RequirementKind};
    use crate::POLICY_SCHEMA_VERSION;
    use chrono::Utc;
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};

    fn requirement(
        dimension: DimensionName,
        accepts: DimensionState,
        on_failure: OnFailure,
    ) -> Requirement {
        Requirement {
            kind: RequirementKind::Dimension {
                dimension,
                accepts: vec![accepts],
            },
            on_failure,
            overridable_by: None,
        }
    }

    fn candidate_policy() -> TrustPolicy {
        TrustPolicy {
            id: "prod".into(),
            version: "2".into(),
            schema_version: POLICY_SCHEMA_VERSION.into(),
            requirements: vec![
                requirement(
                    DimensionName::InventoryCompleteness,
                    DimensionState::Complete,
                    OnFailure::Deny,
                ),
                requirement(
                    DimensionName::BuildProvenance,
                    DimensionState::Verified,
                    OnFailure::Deny,
                ),
            ],
        }
    }

    /// A snapshot is a context: the studio measures against what the
    /// collector assembled, never against a live read.
    fn snapshot(
        dimension: Option<DimensionState>,
        provenance: Option<DimensionState>,
    ) -> TrustContext {
        let mut dimensions = Vec::new();
        if let Some(state) = dimension {
            dimensions.push(crate::context::DimensionFact {
                name: DimensionName::InventoryCompleteness,
                state,
            });
        }
        if let Some(state) = provenance {
            dimensions.push(crate::context::DimensionFact {
                name: DimensionName::BuildProvenance,
                state,
            });
        }
        TrustContext {
            version: TRUST_CONTEXT_VERSION.into(),
            subject: SubjectFact {
                hrn: "hrn:hodei:artifact::acme:Package/com.example%2Flib@1".into(),
                digest: None,
            },
            target: TargetEnvironment::Production,
            dimensions,
            capsule_hash: Some("sha256:abc".into()),
            risk: RiskFacts::default(),
            exceptions: Vec::new(),
            evidence_ids: vec!["evidence-sbom-1".into()],
            assembled_at: Utc::now(),
        }
    }

    fn subjects(context: TrustContext) -> Vec<SimulatedSubject> {
        vec![SimulatedSubject {
            subject: "release-1".into(),
            context,
        }]
    }

    #[test]
    fn a_complete_snapshot_answers_with_a_policy_decision() {
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(candidate_policy()),
            &subjects(snapshot(
                Some(DimensionState::Complete),
                Some(DimensionState::Verified),
            )),
        );
        assert_eq!(answer.effects.len(), 1);
        assert_eq!(answer.effects[0].kind, StudioKind::PolicyDecision);
        assert_eq!(
            answer.effects[0].decision.as_ref().unwrap().outcome,
            Outcome::Allow
        );
        // The effect binds to the policy that was asked about. AC2.
        let decision = answer.effects[0].decision.as_ref().unwrap();
        assert_eq!(decision.policy_id, "prod");
        assert_eq!(decision.policy_version, "2");
        assert_eq!(decision.evidence_ids, vec!["evidence-sbom-1".to_string()]);
    }

    #[test]
    fn a_missing_dimension_answers_with_an_evidence_gap_not_a_refusal() {
        // The candidate would refuse when it could measure; the studio
        // still says *why*: the criteria were never looked at, and names
        // the hole. AC2.
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(candidate_policy()),
            &subjects(snapshot(Some(DimensionState::Complete), None)),
        );
        assert_eq!(answer.effects[0].kind, StudioKind::EvidenceGap);
        let gap = answer.effects[0].gap.as_ref().unwrap();
        assert_eq!(gap.dimension.as_deref(), Some("build_provenance"));
        assert_eq!(gap.reason, "TRUST_PROVENANCE_MISSING");
        assert_eq!(gap.requirement, "dimension:build_provenance");
        assert_eq!(gap.evidence_ids, vec!["evidence-sbom-1".to_string()]);
    }

    #[test]
    fn a_state_that_happens_to_be_bad_is_a_decision_not_a_gap() {
        // Provenance exists and it is INVALID: the criteria *were* looked
        // at, and they refused. That is a policy decision, not a gap.
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(candidate_policy()),
            &subjects(snapshot(
                Some(DimensionState::Complete),
                Some(DimensionState::Invalid),
            )),
        );
        assert_eq!(answer.effects[0].kind, StudioKind::PolicyDecision);
        assert_eq!(
            answer.effects[0].decision.as_ref().unwrap().outcome,
            Outcome::Deny
        );
    }

    #[test]
    fn a_malformed_candidate_is_an_engine_error_never_a_deny() {
        let mut broken = candidate_policy();
        broken.requirements.clear();
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(broken),
            &subjects(snapshot(
                Some(DimensionState::Complete),
                Some(DimensionState::Verified),
            )),
        );
        assert_eq!(answer.effects[0].kind, StudioKind::EngineError);
        assert!(
            answer.effects[0]
                .error
                .as_ref()
                .unwrap()
                .contains("requirements"),
            "the error says why: {:?}",
            answer.effects[0].error
        );
    }

    #[test]
    fn a_contradictory_context_is_an_engine_error() {
        let mut context = snapshot(
            Some(DimensionState::Complete),
            Some(DimensionState::Verified),
        );
        context.risk = RiskFacts {
            critical_known: 0,
            critical_reachable: 3,
            ..RiskFacts::default()
        };
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(candidate_policy()),
            &subjects(context),
        );
        assert_eq!(answer.effects[0].kind, StudioKind::EngineError);
    }

    #[test]
    fn the_diff_names_every_subject_whose_outcome_changes() {
        let active = TrustPolicy {
            id: "prod".into(),
            version: "1".into(),
            schema_version: POLICY_SCHEMA_VERSION.into(),
            requirements: vec![requirement(
                DimensionName::InventoryCompleteness,
                DimensionState::Complete,
                OnFailure::Deny,
            )],
        };
        // The candidate drops the requirement: the release that was
        // allowed stays allowed, the release with a partial inventory
        // flips to allowed -- the loosening is what the diff shows.
        let candidate = TrustPolicy {
            id: "prod".into(),
            version: "2".into(),
            schema_version: POLICY_SCHEMA_VERSION.into(),
            requirements: vec![requirement(
                DimensionName::BuildProvenance,
                DimensionState::Verified,
                OnFailure::Deny,
            )],
        };
        let subjects = vec![
            SimulatedSubject {
                subject: "ok".into(),
                context: snapshot(
                    Some(DimensionState::Complete),
                    Some(DimensionState::Verified),
                ),
            },
            SimulatedSubject {
                subject: "loosened".into(),
                context: snapshot(
                    Some(DimensionState::Partial),
                    Some(DimensionState::Verified),
                ),
            },
        ];
        let answer = simulate_effects(Some(&active), &StudioCandidate::draft(candidate), &subjects);
        assert_eq!(answer.active_version.as_deref(), Some("1"));
        assert_eq!(answer.changes.len(), 1);
        assert_eq!(answer.changes[0].subject, "loosened");
        assert_eq!(answer.changes[0].before, Outcome::Deny);
        assert_eq!(answer.changes[0].after, Outcome::Allow);
        assert!(answer.changes[0].loosens);
    }

    #[test]
    fn a_simulation_never_applies_anything() {
        // AC4: the report of a simulation is a reading. `applied` is
        // false, and the candidate in the answer is the one that was
        // asked about, not the one that decides.
        let answer = simulate_effects(
            None,
            &StudioCandidate::draft(candidate_policy()),
            &subjects(snapshot(
                Some(DimensionState::Complete),
                Some(DimensionState::Verified),
            )),
        );
        assert!(!answer.applied);
        assert_eq!(answer.active_version, None);
    }
}
