//! The SLSA build level as an **evaluation**, never as an existence check.
//!
//! The M2 reading of provenance checked the signature and bound the
//! subject; what a policy could then ask was "is the provenance valid".
//! The question this module adds is the one the standards actually pose:
//! **which level may Hodei assert, and on what did each requirement get
//! its verdict?** A JSON file that says `https://slsa.dev/provenance/v1`
//! asserts nothing by itself: the level is the conjunction of the
//! requirements, and a requirement nobody evaluated is a gap, not a pass.
//!
//! The ladder follows SLSA v1.2 (slsa.dev/spec/v1.2):
//!
//! - **L1** -- the provenance exists as a readable SLSA statement and it
//!   is signed.
//! - **L2** -- L1 and the subject the provenance names is the artifact
//!   itself, the builder the provenance names is one the policy trusts,
//!   and the signing root is pinned to the expected issuer.
//! - **L3** -- L2 and the provenance is non-falsifiable (the builder's own
//!   identity signed it, not a proxy that could say anything about
//!   anything) and the build was hermetic.
//!
//! The requirements this deployment cannot judge offline -- the
//! platform's own protections, reproducibility -- are reported as
//! **`NotEvaluated` with the gap named**. A level whose requirements
//! include a `NotEvaluated` is not asserted. That is the AC: the JSON's
//! existence buys nothing; the verdicts buy the level; and the gaps are
//! visible instead of silently green.

use crate::domain::standards::attestation::{
    AttestationVerification, SLSA_PROVENANCE_PREDICATE_TYPE, SignatureFailure,
};
use crate::standards::trust_chain::{ChainVerification, ExpectedIdentity};
use serde::{Deserialize, Serialize};

/// The version of the SLSA assessor.
pub const SLSA_ASSESSMENT_VERSION: &str = "1.0.0";

/// What the deployment expects of the build the provenance describes.
///
/// The expectations are the policy's own words -- who the trusted builders
/// are, who issued the root, and what the deployment separately knows --
/// and the assessment refuses what the expectations do not cover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlsaExpectations {
    /// The builder identities the policy trusts to have produced a real
    /// hosted build (the trusted-builder list).
    pub trusted_builder_ids: Vec<String>,
    /// The issuer the signing root had to be pinned to. `None` when the
    /// deployment pins no issuer; an unpinned issuer cannot satisfy L2's
    /// issuer requirement.
    pub pinned_issuer: Option<String>,
    /// Whether the deployment has established -- elsewhere, not from this
    /// JSON -- that the signing identity belongs to the builder platform
    /// itself, which is what makes the provenance non-falsifiable.
    pub builder_identity_proven: bool,
    /// Whether the deployment has established that the build is hermetic
    /// beyond what the statement declares. The declared dependencies are
    /// checked directly; this covers what the statement cannot say.
    pub hermetic_declared_complete: bool,
}

/// The SLSA build levels, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SlsaBuildLevel {
    /// Provenance exists and is signed.
    L1,
    /// L1 with the subject and the build identity verified.
    L2,
    /// L2 with non-falsifiable provenance and a hermetic build.
    L3,
}

/// The requirements of the ladder, as this assessor can judge them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlsaRequirement {
    /// The statement reads as an in-toto SLSA v1 provenance.
    ProvenanceAvailable,
    /// The PAE signature holds over the statement.
    ProvenanceSigned,
    /// The subject the provenance names is the artifact the verification
    /// was offered for.
    SubjectVerified,
    /// The builder the provenance names is one the policy trusts.
    BuilderTrusted,
    /// The signing root is pinned to the expected issuer.
    IssuerPinned,
    /// The provenance could not have been falsified: the builder
    /// platform's own identity signed it.
    NonFalsifiable,
    /// The build was hermetic: every input is declared with a digest.
    HermeticBuild,
}

impl SlsaRequirement {
    /// The level that turns on this requirement.
    pub fn minimum_level(&self) -> SlsaBuildLevel {
        use SlsaRequirement::{
            BuilderTrusted, HermeticBuild, IssuerPinned, NonFalsifiable, ProvenanceAvailable,
            ProvenanceSigned, SubjectVerified,
        };
        match self {
            ProvenanceAvailable | ProvenanceSigned => SlsaBuildLevel::L1,
            SubjectVerified | BuilderTrusted | IssuerPinned => SlsaBuildLevel::L2,
            NonFalsifiable | HermeticBuild => SlsaBuildLevel::L3,
        }
    }
}

/// The verdict of one requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    /// It was evaluated and it holds.
    Met,
    /// It was evaluated and it does not hold, with the reason.
    NotMet { reason: String },
    /// It could not be evaluated from this material, and the gap says
    /// what would be needed. A gap is not a pass and never becomes one
    /// by being ignored.
    NotEvaluated { gap: String },
}

impl RequirementStatus {
    /// Whether the requirement holds.
    pub fn is_met(&self) -> bool {
        matches!(self, Self::Met)
    }

    /// The gap, when the requirement was not evaluated.
    pub fn gap(&self) -> Option<&str> {
        match self {
            Self::NotEvaluated { gap } => Some(gap),
            _ => None,
        }
    }
}

/// One requirement with its verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlsaFinding {
    /// The requirement.
    pub requirement: SlsaRequirement,
    /// What the evaluation reached.
    pub status: RequirementStatus,
}

/// The assessment of one provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlsaAssessment {
    /// The version of the assessor that produced it.
    pub assessor_version: String,
    /// Every requirement that takes part in the ladder, with its verdict.
    pub findings: Vec<SlsaFinding>,
}

impl SlsaAssessment {
    /// The highest level whose every requirement was evaluated and met.
    ///
    /// A level with a `NotEvaluated` underneath it is not asserted: the
    /// requirements were not all evaluated, and the AC is exact about
    /// that. The gaps make the refusal readable.
    pub fn asserted_level(&self) -> Option<SlsaBuildLevel> {
        [SlsaBuildLevel::L1, SlsaBuildLevel::L2, SlsaBuildLevel::L3]
            .into_iter()
            .filter(|level| {
                self.level_requirements(*level)
                    .iter()
                    .all(|finding| finding.status.is_met())
            })
            .max()
    }

    /// The findings that take part in a level and below.
    fn level_requirements(&self, level: SlsaBuildLevel) -> Vec<&SlsaFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.requirement.minimum_level() <= level)
            .collect()
    }

    /// The requirements of a level that did not hold.
    pub fn unmet_at(&self, level: SlsaBuildLevel) -> Vec<&SlsaFinding> {
        self.level_requirements(level)
            .into_iter()
            .filter(|finding| matches!(finding.status, RequirementStatus::NotMet { .. }))
            .collect()
    }

    /// The gaps: what this assessment could not evaluate and therefore
    /// does not assert past.
    pub fn gaps(&self) -> Vec<&SlsaFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.status.gap().is_some())
            .collect()
    }

    /// Whether a level may be asserted; the reason is the whole finding
    /// list, which the caller can show.
    pub fn may_assert(&self, level: SlsaBuildLevel) -> bool {
        self.asserted_level()
            .is_some_and(|asserted| asserted >= level)
    }
}

/// Evaluates the provenance behind a chain verification against the
/// expectations, given the identity the chain was asked to confirm.
///
/// The subject and the signature are not re-verified here -- the chain
/// ran them once -- but they are read apart: the **bytes** may verify
/// (`cryptographic_holds`) while the **statement** speaks about a
/// different artifact, and the ladder turns on the second fact too. What
/// the assessor adds is the ladder over those verdicts and over the
/// build identity, and it is exact about what it could not judge.
pub fn assess_provenance(
    chain: &ChainVerification,
    expected: &ExpectedIdentity,
    expectations: &SlsaExpectations,
) -> SlsaAssessment {
    let attestation = &chain.attestation;
    let mut findings = Vec::new();

    // ProvenanceAvailable: the envelope carried a readable SLSA v1
    // statement. The existence of the JSON file is not the test -- the
    // predicate type, the statement type and a non-empty subject are.
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::ProvenanceAvailable,
        status: if attestation.statement.predicate_type == SLSA_PROVENANCE_PREDICATE_TYPE
            && !attestation.statement.subject.is_empty()
        {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotMet {
                reason: format!(
                    "the predicate type is {:?} and the subject list has {} entries",
                    attestation.statement.predicate_type,
                    attestation.statement.subject.len()
                ),
            }
        },
    });

    // ProvenanceSigned: the verdict of the bytes, kept apart from what
    // the policy did later.
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::ProvenanceSigned,
        status: if chain.cryptographic_holds {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotMet {
                reason: attestation.verification.to_string(),
            }
        },
    });

    // SubjectVerified: the statement names the artifact the verification
    // was offered for. A wrong-digest bundle fails here even though its
    // signature holds -- that is the check the AC's "subject verified"
    // refers to.
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::SubjectVerified,
        status: if chain
            .attestation
            .speaks_about(&expected.subject_algorithm, &expected.subject_digest)
        {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotMet {
                reason: format!(
                    "the statement does not name {}:{}",
                    expected.subject_algorithm, expected.subject_digest
                ),
            }
        },
    });

    // BuilderTrusted: the trusted-builder policy is data, and this is
    // where it is read. An empty list is an untrusted list.
    let builder = attestation.statement.predicate.builder_id.as_str();
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::BuilderTrusted,
        status: if builder.is_empty() {
            RequirementStatus::NotMet {
                reason: "the provenance names no builder".to_string(),
            }
        } else if expectations
            .trusted_builder_ids
            .iter()
            .any(|trusted| trusted == builder)
        {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotMet {
                reason: format!("the builder {builder} is not in the trusted builder list"),
            }
        },
    });

    // IssuerPinned: the chain enforced the pin when it was asked to, and
    // it reached the issuer check only after the signature and the
    // subject held. A chain that verified without an issuer expectation
    // proves nothing about the issuer, and a chain that refused BEFORE
    // the issuer check (a bad signature, a revoked root, the wrong
    // subject) never tested the pin at all. Both are `NotEvaluated`, not
    // `Met` and not a false `NotMet`: the assessor separates "the pin
    // failed" from "the pin was never reached".
    let pin_reached_and_held = matches!(attestation.verification, AttestationVerification::Verified { .. })
        // The builder mismatch is recorded last and does not stop the
        // issuer check, so a chain refused for the builder still proved
        // the issuer pin.
        || matches!(
            &attestation.verification,
            AttestationVerification::Invalid {
                reason: SignatureFailure::IdentityMismatch { expected, .. }
            } if expected != expectations.pinned_issuer.as_deref().unwrap_or_default()
        );
    let pin_failed = matches!(
        &attestation.verification,
        AttestationVerification::Invalid {
            reason: SignatureFailure::IdentityMismatch { expected, .. }
        } if Some(expected.as_str()) == expectations.pinned_issuer.as_deref()
    );
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::IssuerPinned,
        status: match &expectations.pinned_issuer {
            None => RequirementStatus::NotEvaluated {
                gap: "the deployment pins no issuer, so L2's issuer requirement cannot be met".to_string(),
            },
            Some(pinned) if expected.issuer.is_none() => RequirementStatus::NotEvaluated {
                gap: format!(
                    "the chain was run without an issuer pin to {pinned}, so pinning was not verified"
                ),
            },
            _ if pin_reached_and_held => RequirementStatus::Met,
            _ if pin_failed => RequirementStatus::NotMet {
                reason: format!(
                    "the signing root is not pinned to {}: {}",
                    expectations.pinned_issuer.as_deref().unwrap_or_default(),
                    attestation.verification
                ),
            },
            Some(pinned) => RequirementStatus::NotEvaluated {
                gap: format!(
                    "the chain refused before the issuer pin to {pinned} could be tested: {}",
                    attestation.verification
                ),
            },
        },
    });

    // NonFalsifiable: what the signature and the pinned root cannot say
    // alone. Offline, the deployment can only bring this as a separate
    // establishment; the JSON cannot buy it.
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::NonFalsifiable,
        status: if expectations.builder_identity_proven
            && chain.cryptographic_holds
            && chain.root_key.is_some()
        {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotEvaluated {
                gap: "the builder platform's own identity as the signer cannot be established from provenance alone".to_string(),
            }
        },
    });

    // HermeticBuild: every declared input must carry a digest, and the
    // statement must not hide inputs. The second half is not sayable
    // offline from the JSON: the deployment can declare the build was
    // hermetic from its own observation, and without that declaration
    // this stays a gap, never a Met.
    let declared_with_digests = attestation
        .statement
        .predicate
        .resolved_dependencies
        .iter()
        .all(|dependency| {
            !dependency.digests.is_empty()
                && dependency.digests.values().all(|digest| !digest.is_empty())
        });
    findings.push(SlsaFinding {
        requirement: SlsaRequirement::HermeticBuild,
        status: if !declared_with_digests {
            RequirementStatus::NotMet {
                reason: "a resolved dependency is declared without a digest".to_string(),
            }
        } else if expectations.hermetic_declared_complete {
            RequirementStatus::Met
        } else {
            RequirementStatus::NotEvaluated {
                gap: "the statement declares digests, but that nothing was left undeclared cannot be read from the JSON".to_string(),
            }
        },
    });

    SlsaAssessment {
        assessor_version: SLSA_ASSESSMENT_VERSION.to_string(),
        findings,
    }
}
