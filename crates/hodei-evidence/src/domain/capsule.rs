//! The trust profile and the trust capsule.
//!
//! A profile that only says "it passes" is worth nothing. What is worth
//! something is a profile that says, dimension by dimension, **which evidence
//! holds the verdict and which is missing**, with its origin and its limits.
//!
//! The capsule is the verifiable passport of a release: a photograph of the
//! state of the trust at a given moment, with its hash and its version, so
//! that two capsules of the same release can be compared and a capsule can
//! be verified again later.
//!
//! # The distinction the whole thing rests on
//!
//! **Present is not verified.** And a dimension without evidence is not
//! filled with a supposition: it is `MISSING`, and `MISSING` is not a
//! favourable state. The threat model is explicit -- "missing evidence is
//! not equivalent to passing evidence" -- and the profile says so in a
//! value that a policy can read.
//!
//! # The states
//!
//! The states are explicit and they are not booleans, because a boolean
//! cannot say "the evidence is there but it is old" apart from "the
//! evidence is there and it holds".
//!
//! - `VERIFIED`: the evidence is there and it was checked, and it holds.
//! - `UNVERIFIED`: the evidence is there and nobody checked it. A
//!   declaration.
//! - `INVALID`: the evidence is there, it was checked, and it does not
//!   hold. A contradiction.
//! - `CONTRADICTORY`: the evidence is there and it says more than one
//!   thing. Every claim stays visible.
//! - `STALE`: the evidence is there and it is older than the threshold.
//! - `MISSING`: there is no evidence at all.
//! - `UNKNOWN`: there is evidence and it does not settle the question.
//! - `COMPLETE` and `PARTIAL`: what an inventory says of itself.
//! - `NOT_APPLICABLE`: the dimension does not apply to this subject.

use crate::domain::content::ContentDigest;
use crate::domain::evidence::{
    EvidenceId, EvidenceType, EvidenceVerification, SourceRef, VerificationStatus,
};
use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The name of a dimension of the trust profile.
///
/// The list is the one the plan names. A dimension this version does not
/// know keeps its name instead of being forced into one of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionName {
    /// What is known about the integrity of the bytes.
    Integrity,
    /// What is known about the source the artefact came from.
    SourceAssurance,
    /// What is known about the provenance of the build.
    BuildProvenance,
    /// What is known about the builder that built it.
    BuilderTrust,
    /// How complete the inventory is.
    InventoryCompleteness,
    /// What is known about the signatures.
    SignatureState,
    /// How old the evidence is.
    EvidenceFreshness,
    /// What is known about the tests.
    TestEvidence,
    /// What is known about the known vulnerabilities.
    VulnerabilityRisk,
    /// What is known about the security findings.
    SecurityEvidence,
    /// What is known about the reachable vulnerabilities.
    ReachableRisk,
    /// What is known about the exposure at runtime.
    RuntimeExposure,
    /// What is known about the controls.
    ComplianceState,
    /// What exceptions are in force.
    ActiveExceptions,
    /// What the policy decided.
    PolicyState,
    /// A dimension this version of Hodei does not know.
    #[serde(untagged)]
    Other(String),
}

impl DimensionName {
    /// The name as the policy and the traces use it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Integrity => "integrity",
            Self::SourceAssurance => "source_assurance",
            Self::BuildProvenance => "build_provenance",
            Self::BuilderTrust => "builder_trust",
            Self::InventoryCompleteness => "inventory_completeness",
            Self::SignatureState => "signature_state",
            Self::EvidenceFreshness => "evidence_freshness",
            Self::TestEvidence => "test_evidence",
            Self::VulnerabilityRisk => "vulnerability_risk",
            Self::SecurityEvidence => "security_evidence",
            Self::ReachableRisk => "reachable_risk",
            Self::RuntimeExposure => "runtime_exposure",
            Self::ComplianceState => "compliance_state",
            Self::ActiveExceptions => "active_exceptions",
            Self::PolicyState => "policy_state",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for DimensionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The state of one dimension of the trust profile.
///
/// The states are explicit because a boolean cannot say what a policy needs
/// to know: "there is no evidence" is not "the evidence is there and it
/// holds", and neither of them is "the evidence is there but it is old".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DimensionState {
    /// The evidence is there, it was checked, and it holds.
    Verified,
    /// The evidence is there and nobody checked it: it is a declaration.
    Unverified,
    /// The evidence is there, it was checked, and it does not hold.
    Invalid,
    /// The evidence is there and it says more than one thing.
    ///
    /// Every claim stays visible and the derived reading names the rule it
    /// applied. A contradiction is not resolved by dropping one of the
    /// claims.
    Contradictory,
    /// The evidence is there but it is older than the threshold.
    Stale,
    /// The evidence is there and it is current.
    Fresh,
    /// There is no evidence at all.
    ///
    /// This is not a favourable state and it never is.
    Missing,
    /// There is evidence and it does not settle the question.
    Unknown,
    /// The inventory names every component.
    Complete,
    /// The inventory is there and it does not name everything.
    Partial,
    /// The dimension does not apply to this subject.
    #[serde(rename = "NOT_APPLICABLE")]
    NotApplicable,
}

impl DimensionState {
    /// Whether the state is a favourable one.
    ///
    /// Only `VERIFIED`, `COMPLETE` and `FRESH` are. `UNVERIFIED`,
    /// `UNKNOWN`, `STALE`, `CONTRADICTORY` and `MISSING` are not: a
    /// dimension that cannot be established does not count as established.
    pub fn is_favourable(&self) -> bool {
        matches!(self, Self::Verified | Self::Complete | Self::Fresh)
    }

    /// The state as the name the policy and the traces use.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::Unverified => "UNVERIFIED",
            Self::Invalid => "INVALID",
            Self::Contradictory => "CONTRADICTORY",
            Self::Stale => "STALE",
            Self::Fresh => "FRESH",
            Self::Missing => "MISSING",
            Self::Unknown => "UNKNOWN",
            Self::Complete => "COMPLETE",
            Self::Partial => "PARTIAL",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

impl fmt::Display for DimensionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A piece of evidence that holds up a dimension.
///
/// The link carries what a reader needs to go and look: the identity of the
/// evidence, how far its verification goes, and the version of the
/// normalizer that read it --because the same document read by two
/// normalizers is two pieces of evidence and not one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLink {
    /// The identity of the evidence.
    pub evidence_id: EvidenceId,
    /// What kind of evidence it is.
    pub evidence_type: EvidenceType,
    /// How far the verification of it goes.
    pub verification: EvidenceVerification,
    /// The normalizer that read it.
    pub normalizer: String,
    /// The version of that normalizer.
    pub normalizer_version: String,
    /// Where it came from, outside Hodei.
    pub source: SourceRef,
    /// When Hodei learned about it.
    pub observed_at: DateTime<Utc>,
}

impl EvidenceLink {
    /// The link to a piece of evidence, built from its record.
    pub fn of(evidence: &crate::domain::evidence::EvidenceRecord) -> Self {
        Self {
            evidence_id: evidence.id().clone(),
            evidence_type: evidence.evidence_type().clone(),
            verification: evidence.verification().clone(),
            normalizer: evidence.normalization().normalizer.clone(),
            normalizer_version: evidence.normalization().version.clone(),
            source: evidence.source().clone(),
            observed_at: evidence.observed_at(),
        }
    }
}

/// One dimension of the trust profile.
///
/// A dimension always says which evidence holds it up, or says that there is
/// none. The absence is a value and not a hole: a dimension with no
/// evidence is `MISSING`, and the reason says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension {
    /// The dimension.
    pub name: DimensionName,
    /// What the state of it is.
    pub state: DimensionState,
    /// The evidence that holds it up. It is empty only when the state is
    /// `MISSING` or `NOT_APPLICABLE`.
    #[serde(default)]
    pub evidence: Vec<EvidenceLink>,
    /// Why the state is what it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Dimension {
    /// A dimension with no evidence at all.
    pub fn missing(name: DimensionName, reason: impl Into<String>) -> Self {
        Self {
            name,
            state: DimensionState::Missing,
            evidence: Vec::new(),
            reason: Some(reason.into()),
        }
    }

    /// A dimension that does not apply to this subject.
    pub fn not_applicable(name: DimensionName) -> Self {
        Self {
            name,
            state: DimensionState::NotApplicable,
            evidence: Vec::new(),
            reason: None,
        }
    }

    /// Whether the dimension may be read as a favourable one.
    ///
    /// The rule the whole profile rests on: a dimension that cannot be
    /// established does not count as established.
    pub fn is_satisfied(&self) -> bool {
        self.state.is_favourable()
    }
}

/// The trust profile of a subject.
///
/// The profile is dimensional on purpose. A single number would hide which
/// dimension holds and which does not, and a decision that needs the second
/// one would have to guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustProfile {
    /// The subject the profile is about.
    pub subject: Hrn,
    /// The dimensions.
    pub dimensions: Vec<Dimension>,
}

impl TrustProfile {
    /// The dimension of that name, when the profile has it.
    pub fn dimension(&self, name: &DimensionName) -> Option<&Dimension> {
        self.dimensions
            .iter()
            .find(|dimension| &dimension.name == name)
    }

    /// The dimensions that are not satisfied.
    ///
    /// This is what a policy reads: what is missing, what is invalid and
    /// what is stale.
    pub fn unsatisfied(&self) -> Vec<&Dimension> {
        self.dimensions
            .iter()
            .filter(|dimension| !dimension.is_satisfied())
            .collect()
    }
}

/// The trust capsule of a subject.
///
/// The capsule is a view, not a stored aggregate: it is assembled from the
/// evidence that is there. What makes it a **capsule** and not a view is
/// the hash: it identifies the exact inputs the reading was made from, so a
/// decision can point at them and a later reader can check that the capsule
/// under a hash still says what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustCapsule {
    /// The subject the capsule is about.
    pub subject: Hrn,
    /// The profile.
    pub profile: TrustProfile,
    /// The version of the assembly.
    pub version: String,
    /// The hash of the inputs the reading was made from.
    pub content_hash: ContentDigest,
    /// When the capsule was assembled.
    ///
    /// The moment is **not** part of the hash: the hash identifies the
    /// inputs, and two assemblies of the same inputs are the same capsule.
    pub assembled_at: DateTime<Utc>,
}

impl TrustCapsule {
    /// The dimensions of the profile of the capsule.
    pub fn dimensions(&self) -> &[Dimension] {
        &self.profile.dimensions
    }
}

/// The version of the assembly of a capsule.
///
/// The version takes part in the hash, so a capsule assembled by a different
/// version is a different capsule and the two cannot be compared as if they
/// were the same reading.
pub const CAPSULE_VERSION: &str = "1.0.0";

/// The state of a dimension that has evidence, read from the evidence.
///
/// The function is the rule that turns the verification of a piece of
/// evidence into the state of a dimension. It is a function and not a
/// judgement: it says what the evidence says, and it never says more.
pub fn state_of(verification: &EvidenceVerification) -> DimensionState {
    match verification.status {
        VerificationStatus::Verified => DimensionState::Verified,
        VerificationStatus::Invalid => DimensionState::Invalid,
        VerificationStatus::Unverified => DimensionState::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_and_complete_are_favourable() {
        assert!(DimensionState::Verified.is_favourable());
        assert!(DimensionState::Complete.is_favourable());
        for state in [
            DimensionState::Unverified,
            DimensionState::Invalid,
            DimensionState::Contradictory,
            DimensionState::Stale,
            DimensionState::Missing,
            DimensionState::Unknown,
            DimensionState::Partial,
            DimensionState::NotApplicable,
        ] {
            assert!(
                !state.is_favourable(),
                "{} may not count as established",
                state
            );
        }
    }

    #[test]
    fn the_verification_of_the_evidence_is_the_state_of_the_dimension() {
        assert_eq!(
            state_of(&EvidenceVerification::verified("signature")),
            DimensionState::Verified
        );
        assert_eq!(
            state_of(&EvidenceVerification::invalid("signature")),
            DimensionState::Invalid
        );
        assert_eq!(
            state_of(&EvidenceVerification::unverified()),
            DimensionState::Unverified
        );
    }
}
