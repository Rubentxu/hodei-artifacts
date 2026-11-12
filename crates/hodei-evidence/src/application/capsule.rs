//! The assembly of the trust capsule.
//!
//! The assembler takes the evidence that is there and builds the profile and
//! the capsule. It is **deterministic**: the same evidence produces the same
//! capsule, byte for byte, and the moment of the assembly is not part of it.
//!
//! # The hash covers the inputs, not the moment
//!
//! That is the whole point of the hash. A decision that points at a capsule
//! points at the exact evidence the reading was made from, and a later
//! reader can check that the capsule under that hash still says what it
//! said. If the moment of the assembly were part of the hash, two readers
//! of the same evidence would get two different capsules and could not
//! compare them.
//!
//! And the version of the assembly **is** part of it: a capsule assembled
//! by another version is another reading, and the two are not the same
//! capsule.

use crate::domain::capsule::{
    CAPSULE_VERSION, Dimension, DimensionName, DimensionState, EvidenceLink, TrustCapsule,
    TrustProfile,
};
use crate::domain::content::ContentDigest;
use crate::domain::evidence::EvidenceRecord;
use chrono::{DateTime, Duration, Utc};
use kernel::Hrn;
use std::collections::BTreeMap;

/// How old the evidence may be before it is stale.
///
/// The threshold is a parameter and not a constant, because "how old is too
/// old" depends on what is being judged: a signature and a scan do not age
/// at the same rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessThreshold {
    /// How old the evidence may be.
    pub maximum_age: Duration,
}

impl Default for FreshnessThreshold {
    fn default() -> Self {
        Self {
            maximum_age: Duration::days(30),
        }
    }
}

/// The assembler of the trust capsule.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapsuleAssembler {
    freshness: FreshnessThreshold,
}

impl CapsuleAssembler {
    /// An assembler with the default freshness threshold.
    pub fn new() -> Self {
        Self::default()
    }

    /// An assembler with a freshness threshold of its own.
    pub fn with_freshness(freshness: FreshnessThreshold) -> Self {
        Self { freshness }
    }

    /// The freshness threshold the assembler uses.
    pub fn freshness(&self) -> FreshnessThreshold {
        self.freshness
    }

    /// Assembles the capsule of a subject from the evidence about it.
    ///
    /// The evidence is the evidence that is there. The assembler does not
    /// look for more, and it does not fill a gap: a dimension with no
    /// evidence is `MISSING`, and the reason says so.
    ///
    /// The dimensions that M2 covers are **always** there, with or without
    /// evidence, because a profile that only shows the dimensions that
    /// happen to have evidence cannot answer what is missing.
    pub fn assemble(
        &self,
        subject: &Hrn,
        evidence: &[EvidenceRecord],
        now: DateTime<Utc>,
    ) -> TrustCapsule {
        let mut by_dimension: BTreeMap<DimensionName, Vec<EvidenceLink>> = BTreeMap::new();

        for record in evidence {
            let dimension = dimension_of(record);
            by_dimension
                .entry(dimension)
                .or_default()
                .push(EvidenceLink::of(record));
        }

        // The dimensions that M2 covers are always part of the profile.
        for dimension in DIMENSIONS_COVERED {
            by_dimension.entry(dimension.clone()).or_default();
        }

        let mut dimensions = Vec::with_capacity(by_dimension.len());
        for (name, links) in &by_dimension {
            dimensions.push(self.dimension_of(name.clone(), links.clone(), now));
        }

        // The freshness of the evidence is its own dimension, and it says
        // whether the reading is current at all.
        let oldest = evidence.iter().map(|record| record.observed_at()).min();
        let freshness = match oldest {
            Some(oldest) if now.signed_duration_since(oldest) > self.freshness.maximum_age => {
                Dimension {
                    name: DimensionName::EvidenceFreshness,
                    state: DimensionState::Stale,
                    evidence: Vec::new(),
                    reason: Some(format!(
                        "the oldest evidence is {} old and the threshold is {}",
                        humandays(now.signed_duration_since(oldest)),
                        humandays(self.freshness.maximum_age)
                    )),
                }
            }
            Some(_) => Dimension {
                name: DimensionName::EvidenceFreshness,
                state: DimensionState::Fresh,
                evidence: Vec::new(),
                reason: None,
            },
            None => Dimension::missing(
                DimensionName::EvidenceFreshness,
                "there is no evidence, and so there is nothing to be fresh about",
            ),
        };
        dimensions.push(freshness);

        let profile = TrustProfile {
            subject: subject.clone(),
            dimensions,
        };

        let content_hash = hash_of_inputs(&profile, evidence);

        TrustCapsule {
            subject: subject.clone(),
            profile,
            version: CAPSULE_VERSION.to_string(),
            content_hash,
            assembled_at: now,
        }
    }

    /// The state of one dimension, read from the evidence that holds it up.
    ///
    /// The rules are fixed and they are few. The most consequential fact
    /// wins, and the reason says what happened:
    ///
    /// - A piece of evidence that was checked and does not hold, together
    ///   with another that was checked and holds: the two contradict each
    ///   other, and **both stay visible**.
    /// - A piece of evidence that was checked and does not hold, and
    ///   nothing that holds: `INVALID`.
    /// - Evidence that is older than the threshold: `STALE`. An old reading
    ///   is not a current one, and the historical reading stays visible.
    /// - Evidence that was not checked: `UNVERIFIED`. A declaration is not
    ///   a proof.
    /// - Evidence that was checked and holds: `VERIFIED`.
    ///
    /// The inventory is a dimension of its own kind: what it says of itself
    /// is whether it names everything, and that is `COMPLETE` or `PARTIAL`
    /// and not `VERIFIED`.
    fn dimension_of(
        &self,
        name: DimensionName,
        mut links: Vec<EvidenceLink>,
        now: DateTime<Utc>,
    ) -> Dimension {
        if links.is_empty() {
            return Dimension::missing(name, "there is no evidence for this dimension");
        }

        // The links are ordered, so that the capsule does not depend on the
        // order the evidence arrived in.
        links.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));

        let holds = links.iter().any(|link| {
            link.verification.status == crate::domain::evidence::VerificationStatus::Verified
        });
        let does_not_hold = links.iter().any(|link| {
            link.verification.status == crate::domain::evidence::VerificationStatus::Invalid
        });
        let unchecked = links.iter().any(|link| {
            link.verification.status == crate::domain::evidence::VerificationStatus::Unverified
        });

        let oldest = links
            .iter()
            .map(|link| link.observed_at)
            .min()
            .unwrap_or(now);
        let age = now.signed_duration_since(oldest);
        let old = age > self.freshness.maximum_age;

        let state = if does_not_hold && holds {
            DimensionState::Contradictory
        } else if does_not_hold {
            DimensionState::Invalid
        } else if name == DimensionName::InventoryCompleteness {
            // What an inventory says of itself is how complete it is, and
            // that is not the same question as whether it was verified.
            if old {
                DimensionState::Stale
            } else if holds {
                DimensionState::Complete
            } else {
                DimensionState::Partial
            }
        } else if old {
            DimensionState::Stale
        } else if holds {
            DimensionState::Verified
        } else if unchecked {
            DimensionState::Unverified
        } else {
            DimensionState::Unknown
        };

        let reason = match state {
            DimensionState::Contradictory => Some(
                "one of the pieces of evidence was checked and it does not hold, and another one \
                 was checked and it does; both claims stay visible"
                    .to_string(),
            ),
            DimensionState::Invalid => {
                Some("the evidence was checked and it does not hold".to_string())
            }
            DimensionState::Stale => Some(format!(
                "the oldest evidence is {} old and the threshold is {}",
                humandays(age),
                humandays(self.freshness.maximum_age)
            )),
            DimensionState::Unverified => Some("the evidence was not checked".to_string()),
            DimensionState::Partial => {
                Some("the inventory is there and it does not name every component".to_string())
            }
            DimensionState::Unknown => {
                Some("the evidence does not settle the question".to_string())
            }
            _ => None,
        };

        Dimension {
            name,
            state,
            evidence: links,
            reason,
        }
    }
}

/// The dimensions that M2 covers.
///
/// They are always part of the profile, with or without evidence: a
/// profile that only shows the dimensions that happen to have evidence
/// cannot answer the question that matters, which is what is missing.
pub const DIMENSIONS_COVERED: &[DimensionName] = &[
    DimensionName::Integrity,
    DimensionName::SourceAssurance,
    DimensionName::BuildProvenance,
    DimensionName::BuilderTrust,
    DimensionName::InventoryCompleteness,
    DimensionName::SignatureState,
];

/// The dimension a piece of evidence holds up.
///
/// The mapping is the one the domain model of the plan names: the evidence
/// of the integrity of the bytes holds the integrity dimension up, the
/// provenance holds the provenance one, and so on.
pub fn dimension_of(record: &EvidenceRecord) -> DimensionName {
    use crate::domain::evidence::EvidenceType;

    match record.evidence_type() {
        EvidenceType::ArtifactIntegrity => DimensionName::Integrity,
        EvidenceType::SourceMetadata => DimensionName::SourceAssurance,
        EvidenceType::BuildMetadata => DimensionName::BuilderTrust,
        EvidenceType::Provenance => DimensionName::BuildProvenance,
        EvidenceType::Sbom => DimensionName::InventoryCompleteness,
        EvidenceType::SignatureVerification => DimensionName::SignatureState,
        EvidenceType::TestResult => DimensionName::TestEvidence,
        EvidenceType::VulnerabilityIntelligence => DimensionName::VulnerabilityRisk,
        EvidenceType::DeploymentObservation => DimensionName::RuntimeExposure,
        EvidenceType::HumanApproval => DimensionName::ActiveExceptions,
        EvidenceType::Other(name) => match name.as_str() {
            "vex" => DimensionName::VulnerabilityRisk,
            "scan_result" => DimensionName::VulnerabilityRisk,
            "secret_finding" => DimensionName::SecurityEvidence,
            "iac_finding" => DimensionName::SecurityEvidence,
            _ => DimensionName::Other(name.clone()),
        },
        _ => DimensionName::Other(record.evidence_type().to_string()),
    }
}

/// The hash of the inputs of a reading.
///
/// The hash covers the evidence that was read --its identity, its content
/// digest, how far its verification goes and the version of the normalizer
/// that read it-- and the states the reading produced. It does **not** cover
/// the moment of the assembly: two readers of the same evidence get the
/// same capsule.
pub fn hash_of_inputs(profile: &TrustProfile, evidence: &[EvidenceRecord]) -> ContentDigest {
    let mut bytes = Vec::new();

    bytes.extend_from_slice(b"hodei.trust_capsule.v1\n");
    bytes.extend_from_slice(CAPSULE_VERSION.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(profile.subject.to_string().as_bytes());
    bytes.push(b'\n');

    // The dimensions, in the order they are named, so that the hash does
    // not depend on the order the evidence arrived in.
    let mut dimensions: Vec<&Dimension> = profile.dimensions.iter().collect();
    dimensions.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));

    for dimension in dimensions {
        bytes.extend_from_slice(dimension.name.as_str().as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(dimension.state.as_str().as_bytes());
        bytes.push(b'\n');

        let mut links: Vec<&EvidenceLink> = dimension.evidence.iter().collect();
        links.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));

        for link in links {
            bytes.extend_from_slice(link.evidence_id.as_str().as_bytes());
            bytes.push(b'|');
            bytes.extend_from_slice(link.verification.status.to_string().as_bytes());
            bytes.push(b'|');
            bytes.extend_from_slice(link.verification.method.as_deref().unwrap_or("").as_bytes());
            bytes.push(b'|');
            bytes.extend_from_slice(link.normalizer.as_bytes());
            bytes.push(b'|');
            bytes.extend_from_slice(link.normalizer_version.as_bytes());
            bytes.push(b'|');
            bytes.extend_from_slice(link.source.external_id.as_deref().unwrap_or("").as_bytes());
            bytes.push(b'\n');
        }
    }

    // The evidence that was read and did not hold up any dimension is part
    // of the reading too: a capsule that forgot it would not say the same
    // thing twice.
    let mut others: Vec<&EvidenceRecord> = evidence
        .iter()
        .filter(|record| {
            !profile.dimensions.iter().any(|dimension| {
                dimension
                    .evidence
                    .iter()
                    .any(|link| link.evidence_id == *record.id())
            })
        })
        .collect();
    others.sort_by(|left, right| left.id().cmp(right.id()));

    for record in others {
        bytes.extend_from_slice(record.id().as_str().as_bytes());
        bytes.push(b'|');
        bytes.extend_from_slice(record.content().digest.to_string().as_bytes());
        bytes.push(b'\n');
    }

    ContentDigest::of_bytes(&bytes)
}

/// The age in a form a person reads.
fn humandays(duration: Duration) -> String {
    format!("{} days", duration.num_days())
}
