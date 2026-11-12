//! The decision, and the evaluation that produces it.
//!
//! A decision is **immutable**. It is written once, when it is made, and it
//! keeps saying what it said: the outcome, the reason codes, the versions
//! of everything that took part in it, and the evidence that was used.
//!
//! The evaluation is deterministic. The same context and the same policy
//! give the same decision, every time, on any machine. That is what makes
//! the decision reproducible, and it is what lets a policy be changed
//! without rewriting the past.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::policy::{OnFailure, RequirementKind, TrustPolicy};
use crate::reasons::ReasonCode;

/// How a decision came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Outcome {
    /// The promotion may go ahead.
    Allow,
    /// The promotion may not go ahead.
    Deny,
    /// The promotion may go ahead, and there is something to say about it.
    ///
    /// `WARN` is not an `Allow` in disguise: the target workflow decides
    /// explicitly whether a warning can advance, and a decision that
    /// warned and advanced anyway says so in its trace.
    Warn,
    /// The promotion needs somebody's approval.
    RequireApproval,
}

impl Outcome {
    /// Whether the outcome lets the promotion go ahead on its own.
    pub fn advances(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// The name of the outcome.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
            Self::Warn => "WARN",
            Self::RequireApproval => "REQUIRE_APPROVAL",
        }
    }
}

/// What happened to one requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementOutcome {
    /// The name of the requirement.
    pub requirement: String,
    /// Whether it was met.
    pub satisfied: bool,
    /// Why it was not met.
    pub reason: Option<ReasonCode>,
    /// The exception that was used, if there was one.
    pub exception: Option<String>,
}

/// The decision of a trust evaluation.
///
/// It carries everything that a reader needs to replay it: the outcome,
/// the reason codes, the version of the context builder, the version of
/// the policy and of its schema, the hash of the capsule, and the evidence
/// that was used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustDecision {
    /// How it came out.
    pub outcome: Outcome,
    /// Why, in codes.
    pub reasons: Vec<ReasonCode>,
    /// What happened to each requirement.
    pub requirements: Vec<RequirementOutcome>,
    /// The version of the builder of the context.
    pub context_version: String,
    /// The identifier of the policy set.
    pub policy_id: String,
    /// The version of the policy set.
    pub policy_version: String,
    /// The version of the schema of the policy set.
    pub schema_version: String,
    /// The hash of the capsule the decision was measured against.
    pub capsule_hash: Option<String>,
    /// The identifiers of the evidence that was used.
    pub evidence_ids: Vec<String>,
    /// The moment the decision was made.
    pub decided_at: DateTime<Utc>,
}

impl TrustDecision {
    /// Whether the decision lets the promotion go ahead on its own.
    pub fn advances(&self) -> bool {
        self.outcome.advances()
    }

    /// Whether the decision carries a reason.
    pub fn has_reason(&self, reason: ReasonCode) -> bool {
        self.reasons.contains(&reason)
    }
}

/// Evaluates a policy against a context.
///
/// The result is a decision and nothing else: it does not change the
/// context, it does not change the policy, and it does not go and look for
/// anything. Everything it says was in the two things it was given.
pub fn evaluate(context: &TrustContext, policy: &TrustPolicy) -> TrustDecision {
    let mut reasons: Vec<ReasonCode> = Vec::new();
    let mut outcomes: Vec<RequirementOutcome> = Vec::new();
    let mut blocking = false;
    let mut needs_approval = false;
    let mut warned = false;

    for requirement in &policy.requirements {
        let reading = read(&requirement.kind, context);

        if reading.satisfied {
            outcomes.push(RequirementOutcome {
                requirement: requirement.name(),
                satisfied: true,
                reason: None,
                exception: None,
            });
            continue;
        }

        // An exception may waive the requirement, and it has to be the one
        // that was named, it has to be active and it has to apply to this
        // subject. An exception that expired cannot be reused.
        let waived = requirement.overridable_by.as_ref().and_then(|name| {
            context.exceptions.iter().find(|exception| {
                &exception.name == name && exception.is_active_at(context.assembled_at)
            })
        });

        if let Some(exception) = waived {
            outcomes.push(RequirementOutcome {
                requirement: requirement.name(),
                satisfied: true,
                reason: None,
                exception: Some(exception.id.clone()),
            });
            reasons.push(ReasonCode::ExceptionApplied);
            warned = true;
            continue;
        }

        let reason = reading.reason.unwrap_or(ReasonCode::EvidenceMissing);
        reasons.push(reason);
        match requirement.on_failure {
            OnFailure::Deny => blocking = true,
            OnFailure::RequireApproval => needs_approval = true,
        }
        outcomes.push(RequirementOutcome {
            requirement: requirement.name(),
            satisfied: false,
            reason: Some(reason),
            exception: None,
        });
    }

    let outcome = if blocking {
        Outcome::Deny
    } else if needs_approval {
        Outcome::RequireApproval
    } else if warned {
        Outcome::Warn
    } else {
        Outcome::Allow
    };

    TrustDecision {
        outcome,
        reasons,
        requirements: outcomes,
        context_version: context.version.clone(),
        policy_id: policy.id.clone(),
        policy_version: policy.version.clone(),
        schema_version: policy.schema_version.clone(),
        capsule_hash: context.capsule_hash.clone(),
        evidence_ids: context.evidence_ids.clone(),
        decided_at: context.assembled_at,
    }
}

/// What the reading of a requirement found.
struct Reading {
    satisfied: bool,
    reason: Option<ReasonCode>,
}

/// Reads a requirement against the context.
fn read(kind: &RequirementKind, context: &TrustContext) -> Reading {
    match kind {
        RequirementKind::Dimension { dimension, accepts } => match context.state_of(dimension) {
            // The dimension is not in the context at all: there is no
            // evidence for it, and that is an absence and not a state that
            // happens to be bad.
            None => Reading {
                satisfied: false,
                reason: Some(missing_reason(dimension)),
            },
            Some(state) if accepts.contains(&state) => Reading {
                satisfied: true,
                reason: None,
            },
            Some(state) => Reading {
                satisfied: false,
                reason: Some(state_reason(dimension, state)),
            },
        },
        RequirementKind::NoReachableCriticalVulnerability => {
            if context.risk.critical_reachable > 0 {
                Reading {
                    satisfied: false,
                    reason: Some(ReasonCode::RiskReachableCriticalVulnerability),
                }
            } else {
                Reading {
                    satisfied: true,
                    reason: None,
                }
            }
        }
        RequirementKind::RiskEstablished => {
            if context.risk.established {
                Reading {
                    satisfied: true,
                    reason: None,
                }
            } else {
                Reading {
                    satisfied: false,
                    reason: Some(ReasonCode::RiskUnknown),
                }
            }
        }
        RequirementKind::ExposureAtMost { exposure } => {
            if context.risk.exposure == *exposure {
                Reading {
                    satisfied: false,
                    reason: Some(ReasonCode::RiskPolicyBlocking),
                }
            } else {
                Reading {
                    satisfied: true,
                    reason: None,
                }
            }
        }
    }
}

/// The reason of a dimension that is not there.
fn missing_reason(dimension: &hodei_evidence::domain::capsule::DimensionName) -> ReasonCode {
    use hodei_evidence::domain::capsule::DimensionName;
    match dimension {
        DimensionName::InventoryCompleteness => ReasonCode::TrustSbomMissing,
        DimensionName::BuildProvenance => ReasonCode::TrustProvenanceMissing,
        DimensionName::SignatureState => ReasonCode::TrustSignatureMissing,
        DimensionName::Integrity => ReasonCode::TrustIntegrityMissing,
        _ => ReasonCode::EvidenceMissing,
    }
}

/// The reason of a dimension that is there and does not hold.
fn state_reason(
    dimension: &hodei_evidence::domain::capsule::DimensionName,
    state: hodei_evidence::domain::capsule::DimensionState,
) -> ReasonCode {
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
    match (dimension, state) {
        (_, DimensionState::Invalid) => match dimension {
            DimensionName::BuildProvenance => ReasonCode::TrustProvenanceInvalid,
            DimensionName::Integrity => ReasonCode::TrustIntegrityInvalid,
            _ => ReasonCode::TrustEvidenceContradictory,
        },
        (_, DimensionState::Unverified) => ReasonCode::TrustSignatureUnverified,
        (_, DimensionState::Stale) => ReasonCode::EvidenceStale,
        (_, DimensionState::Contradictory) => ReasonCode::TrustEvidenceContradictory,
        (_, DimensionState::Partial) => ReasonCode::TrustInventoryIncomplete,
        (_, DimensionState::Missing) => missing_reason(dimension),
        _ => ReasonCode::EvidenceMissing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warning_is_not_an_allow() {
        assert!(!Outcome::Warn.advances());
        assert!(!Outcome::RequireApproval.advances());
        assert!(Outcome::Allow.advances());
    }
}
