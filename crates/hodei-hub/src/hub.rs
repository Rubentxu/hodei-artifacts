//! The trusted artifact hub: quarantine, the trust channel and
//! governed promotion.
//!
//! The milestone's rule is a single, sharp one: **without a trust decision
//! there is no promotion**, and **promoting does not mutate the bytes**. The digest is
//! the identity of the content, and promotion changes the **channel** it
//! lives in, not what is inside.
//!
//! And there is a detail that seems small and is not: a **newly
//! admitted artifact is in quarantine**, always. Quarantine is not an
//! exceptional state: it is the default state, because an artifact without a decision
//! has not said anything yet.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_trust_policy::decision::Outcome;
use serde::{Deserialize, Serialize};

use crate::blob::BlobStore;

/// Where an artifact is.
///
/// The states are those declared by ADR-0015, and they are **verifiable**: an
/// artifact of which Hodei only has the reference is not the same as
/// one that Hodei has and has promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// There is only a reference: Hodei does not have the bytes.
    ReferenceOnly,
    /// Hodei has the bytes, in quarantine.
    Quarantined,
    /// Hodei has the bytes and they are in the trust channel.
    Trusted,
    /// Hodei does not have the bytes.
    Unavailable,
}

/// An artifact of the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubArtifact {
    /// The digest: the identity of the content.
    pub digest: ContentDigest,
    /// Where it is.
    pub availability: Availability,
    /// The external reference, if it has one.
    ///
    /// An artifact that Hodei does not own may be referenced, and that
    /// reference is real without Hodei having to migrate anything.
    pub external_reference: Option<String>,
}

/// The trust decision that governs a promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustVerdict {
    /// The decision's identifier.
    pub decision_id: String,
    /// What was decided.
    pub outcome: Outcome,
    /// The reasons, with codes.
    pub reasons: Vec<String>,
    /// The version of the policy that decided.
    pub policy_version: String,
}

impl TrustVerdict {
    /// A decision that allows.
    pub fn allowing(decision_id: impl Into<String>, policy_version: impl Into<String>) -> Self {
        Self {
            decision_id: decision_id.into(),
            outcome: Outcome::Allow,
            reasons: Vec::new(),
            policy_version: policy_version.into(),
        }
    }

    /// A decision that denies, with its reasons.
    pub fn denying(
        decision_id: impl Into<String>,
        policy_version: impl Into<String>,
        reasons: Vec<String>,
    ) -> Self {
        Self {
            decision_id: decision_id.into(),
            outcome: Outcome::Deny,
            reasons,
            policy_version: policy_version.into(),
        }
    }
}

/// Why it could not be promoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionError {
    /// There is no decision: without a decision there is no promotion.
    NoDecision,
    /// The decision denies.
    Denied {
        /// The decision's identifier.
        decision_id: String,
        /// The reasons.
        reasons: Vec<String>,
    },
    /// The artifact is not in the hub.
    NotInTheHub,
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for PromotionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDecision => {
                write!(
                    formatter,
                    "there is no trust decision: without one there is no promotion"
                )
            }
            Self::Denied {
                decision_id,
                reasons,
            } => write!(
                formatter,
                "the decision {decision_id} denies the promotion: {}",
                reasons.join(", ")
            ),
            Self::NotInTheHub => write!(formatter, "the artifact is not in the hub"),
            Self::Unavailable(reason) => {
                write!(formatter, "the store is not available: {reason}")
            }
        }
    }
}

/// The trace of a promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionAudit {
    /// The promoted digest.
    pub digest: ContentDigest,
    /// The decision that allowed it.
    pub decision_id: String,
    /// The policy version.
    pub policy_version: String,
    /// Who promoted it.
    pub by: String,
    /// When.
    pub at: DateTime<Utc>,
    /// If it was denied, the reasons.
    pub reasons: Vec<String>,
}

/// The trusted artifact hub.
pub struct ArtifactHub {
    store: Arc<dyn BlobStore>,
    artifacts: std::sync::Mutex<std::collections::BTreeMap<String, HubArtifact>>,
    audits: std::sync::Mutex<Vec<PromotionAudit>>,
}

impl ArtifactHub {
    /// The hub over a content store.
    pub fn new(store: Arc<dyn BlobStore>) -> Self {
        Self {
            store,
            artifacts: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            audits: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Admits an artifact and leaves it **in quarantine**.
    ///
    /// The artifact has the bytes, and is not in the trust channel:
    /// until there is a decision, it does not leave there.
    pub async fn ingest(&self, bytes: &[u8]) -> Result<HubArtifact, PromotionError> {
        let digest = self
            .store
            .put(bytes)
            .await
            .map_err(|error| PromotionError::Unavailable(error.to_string()))?;
        let artifact = HubArtifact {
            digest: digest.clone(),
            availability: Availability::Quarantined,
            external_reference: None,
        };
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .insert(digest.value.clone(), artifact.clone());
        Ok(artifact)
    }

    /// Registers an artifact that is **referenced** but not owned.
    ///
    /// This is coexistence with external registries: Hodei knows about the
    /// artifact, cites it and references it, without migrating its bytes or
    /// claiming to have them.
    pub fn reference(
        &self,
        digest: ContentDigest,
        external_reference: impl Into<String>,
    ) -> HubArtifact {
        let artifact = HubArtifact {
            digest: digest.clone(),
            availability: Availability::ReferenceOnly,
            external_reference: Some(external_reference.into()),
        };
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .insert(digest.value.clone(), artifact.clone());
        artifact
    }

    /// Promotes an artifact to the trust channel.
    ///
    /// Promotion **does not copy and does not mutate the bytes**: what changes is the
    /// channel, and the trust channel references **the exact digest**.
    pub async fn promote(
        &self,
        digest: &ContentDigest,
        verdict: Option<&TrustVerdict>,
        by: &str,
        at: DateTime<Utc>,
    ) -> Result<HubArtifact, PromotionError> {
        let mut artifacts = self.artifacts.lock().expect("el hub no esta envenenado");
        let Some(artifact) = artifacts.get_mut(&digest.value.clone()) else {
            return Err(PromotionError::NotInTheHub);
        };

        // Without a decision, there is no promotion.
        let Some(verdict) = verdict else {
            let audit = PromotionAudit {
                digest: digest.clone(),
                decision_id: String::new(),
                policy_version: String::new(),
                by: by.to_string(),
                at,
                reasons: vec!["there is no trust decision".to_string()],
            };
            self.audits
                .lock()
                .expect("el hub no esta envenenado")
                .push(audit);
            return Err(PromotionError::NoDecision);
        };

        if verdict.outcome != Outcome::Allow {
            let audit = PromotionAudit {
                digest: digest.clone(),
                decision_id: verdict.decision_id.clone(),
                policy_version: verdict.policy_version.clone(),
                by: by.to_string(),
                at,
                reasons: verdict.reasons.clone(),
            };
            self.audits
                .lock()
                .expect("el hub no esta envenenado")
                .push(audit);
            return Err(PromotionError::Denied {
                decision_id: verdict.decision_id.clone(),
                reasons: verdict.reasons.clone(),
            });
        }

        // The digest does NOT change: promotion changes the channel.
        artifact.availability = Availability::Trusted;
        let promoted = artifact.clone();

        self.audits
            .lock()
            .expect("el hub no esta envenenado")
            .push(PromotionAudit {
                digest: digest.clone(),
                decision_id: verdict.decision_id.clone(),
                policy_version: verdict.policy_version.clone(),
                by: by.to_string(),
                at,
                reasons: verdict.reasons.clone(),
            });

        Ok(promoted)
    }

    /// An artifact's bytes, verified by its digest.
    pub async fn bytes_of(&self, digest: &ContentDigest) -> Result<Vec<u8>, PromotionError> {
        self.store
            .get(digest)
            .await
            .map_err(|error| PromotionError::Unavailable(error.to_string()))
    }

    /// An artifact of the hub.
    pub fn artifact(&self, digest: &ContentDigest) -> Option<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .get(&digest.value.clone())
            .cloned()
    }

    /// The trust channel: what can be published.
    pub fn trusted(&self) -> Vec<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .values()
            .filter(|artifact| artifact.availability == Availability::Trusted)
            .cloned()
            .collect()
    }

    /// Quarantine: what cannot be published yet.
    pub fn quarantine(&self) -> Vec<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .values()
            .filter(|artifact| artifact.availability == Availability::Quarantined)
            .cloned()
            .collect()
    }

    /// The promotion trace.
    pub fn audits(&self) -> Vec<PromotionAudit> {
        self.audits
            .lock()
            .expect("el hub no esta envenenado")
            .clone()
    }
}
