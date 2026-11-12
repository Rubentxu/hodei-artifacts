//! The identities of a thing outside Hodei, and how sure Hodei is of them.
//!
//! The same piece of software is named differently by everybody: the
//! ecosystem says `pkg:maven/org.example/lib-a@1.2.3`, the registry says
//! `sha256:9f86d0...`, the build says `lib-a-1.2.3.tar.gz`. Hodei needs to
//! correlate those names without pretending they are the same thing when
//! they are not.
//!
//! Two rules shape the whole contract, and both come from the threat model
//! and from the reference standards:
//!
//! - **The uncertainty is exposed, never hidden.** Every mapping carries a
//!   confidence, and the confidence is what a policy reads. A mapping that
//!   could not be confirmed stays a hint.
//! - **A hint is not a verdict.** An unconfirmed or ambiguous mapping may
//!   suggest a relation, but it may not satisfy a trust requirement on its
//!   own. A policy that needs certainty reads [`MappingConfidence::is_verdict`]
//!   and refuses the rest.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The namespace an external identity lives in.
///
/// The namespace is what says what the value means. A digest in the
/// namespace of OCI is an OCI digest, and the same hexadecimal in another
/// namespace is not the same claim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityNamespace {
    /// A Package URL. The package identity of the ecosystem.
    Purl,
    /// The digest of a container image, as OCI names it.
    OciDigest,
    /// The digest of a file, by algorithm.
    ContentDigest,
    /// The coordinates of a package: the ecosystem, the group and the name.
    PackageCoordinates,
    /// A version control URI.
    VcsUri,
    /// An identifier of a resource in an external system.
    ExternalId,
    /// A namespace this version of Hodei does not know.
    #[serde(untagged)]
    Other(String),
}

impl fmt::Display for IdentityNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Purl => f.write_str("purl"),
            Self::OciDigest => f.write_str("oci_digest"),
            Self::ContentDigest => f.write_str("content_digest"),
            Self::PackageCoordinates => f.write_str("package_coordinates"),
            Self::VcsUri => f.write_str("vcs_uri"),
            Self::ExternalId => f.write_str("external_id"),
            Self::Other(value) => f.write_str(value),
        }
    }
}

/// How sure Hodei is that an external identity names the thing it is
/// attached to.
///
/// The four grades are not a scale of "how good" but a scale of "what may
/// be done with it". Only [`MappingConfidence::Confirmed`] and
/// [`MappingConfidence::Deterministic`] may be used as a verdict; the other
/// two are hints, and a hint that nobody confirmed is not evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingConfidence {
    /// The source document says both names for the same thing.
    ///
    /// The document names the component by its PURL *and* by its digest, or
    /// names the subject and its components in one place. There is no
    /// inference here: the document asserted it.
    Confirmed,
    /// A deterministic rule produced the mapping.
    ///
    /// The rule is a canonicalization, and it is versioned, so the same
    /// inputs always produce the same mapping. It is still a mapping and
    /// not an assertion, but it is not a guess either.
    Deterministic,
    /// The mapping is plausible and nobody confirmed it.
    ///
    /// A name and a version that match a known package are a hint. The hint
    /// may point at the right thing and may not.
    Unconfirmed,
    /// More than one thing could be the one.
    ///
    /// The mapping is not resolved and must not be resolved by guessing.
    Ambiguous,
}

/// The order of the confidences is the order of their strength.
///
/// It is written by hand and not derived, because the declaration order of
/// the variants says nothing about how sure they are, and a `min()` over
/// two confidences has to mean "the weaker one".
impl Ord for MappingConfidence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.strength().cmp(&other.strength())
    }
}

impl PartialOrd for MappingConfidence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl MappingConfidence {
    /// How sure the grade is, from the weakest to the strongest.
    fn strength(&self) -> u8 {
        match self {
            Self::Ambiguous => 0,
            Self::Unconfirmed => 1,
            Self::Deterministic => 2,
            Self::Confirmed => 3,
        }
    }
}

impl MappingConfidence {
    /// Whether a mapping with this confidence may be used as a verdict.
    ///
    /// This is the line the threat model draws: "ambiguous mapping remains
    /// unresolved/low confidence" and "no heuristic merge can silently
    /// satisfy high-assurance policy".
    pub fn is_verdict(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Deterministic)
    }

    /// The grade as the name the policy and the traces use.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Deterministic => "deterministic",
            Self::Unconfirmed => "unconfirmed",
            Self::Ambiguous => "ambiguous",
        }
    }
}

impl fmt::Display for MappingConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The identity of a thing outside Hodei.
///
/// The identity is a **mapping**, not the thing: it says "this thing is
/// also known as this, in this namespace", and it carries everything a
/// reviewer needs to judge the claim:
///
/// - the namespace and the value, which say what is claimed;
/// - the evidence the claim came from, which says who said it;
/// - the confidence, which says how sure Hodei is;
/// - the validity, which says when the claim holds;
/// - the version of the canonicalization, which says how the value was
///   derived from what the source said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIdentity {
    /// The namespace the value lives in.
    pub namespace: IdentityNamespace,
    /// The value itself.
    pub value: String,
    /// How sure Hodei is of the mapping.
    pub confidence: MappingConfidence,
    /// The evidence the mapping came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_evidence: Option<String>,
    /// The version of the canonicalization that produced the value.
    pub canonicalization_version: String,
    /// When the mapping starts to hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<DateTime<Utc>>,
    /// When the mapping stops holding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<DateTime<Utc>>,
}

impl ExternalIdentity {
    /// A mapping that the source document asserted.
    pub fn confirmed(
        namespace: IdentityNamespace,
        value: impl Into<String>,
        source_evidence: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            value: value.into(),
            confidence: MappingConfidence::Confirmed,
            source_evidence: Some(source_evidence.into()),
            canonicalization_version: CANONICALIZATION_VERSION.to_string(),
            valid_from: None,
            valid_to: None,
        }
    }

    /// A mapping that a deterministic rule produced.
    pub fn deterministic(
        namespace: IdentityNamespace,
        value: impl Into<String>,
        canonicalization_version: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            value: value.into(),
            confidence: MappingConfidence::Deterministic,
            source_evidence: None,
            canonicalization_version: canonicalization_version.into(),
            valid_from: None,
            valid_to: None,
        }
    }

    /// A mapping that nobody confirmed: a hint, and nothing more.
    pub fn unconfirmed(namespace: IdentityNamespace, value: impl Into<String>) -> Self {
        Self {
            namespace,
            value: value.into(),
            confidence: MappingConfidence::Unconfirmed,
            source_evidence: None,
            canonicalization_version: CANONICALIZATION_VERSION.to_string(),
            valid_from: None,
            valid_to: None,
        }
    }

    /// Whether the mapping may be used as a verdict.
    pub fn is_verdict(&self) -> bool {
        self.confidence.is_verdict()
    }

    /// Whether the mapping says the thing is the same as another.
    ///
    /// The comparison is on the namespace and the value: two mappings of
    /// different namespaces are never the same identity, even when their
    /// values happen to be equal.
    pub fn is_same_as(&self, other: &ExternalIdentity) -> bool {
        self.namespace == other.namespace && self.value == other.value
    }
}

/// The version of the canonicalization of the external identities.
///
/// The version is part of the identity of a normalized fact: a different
/// version is a different reading of the same document, and it takes part
/// in the idempotency key of the evidence.
pub const CANONICALIZATION_VERSION: &str = "1.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_confirmed_or_deterministic_mapping_is_a_verdict() {
        assert!(MappingConfidence::Confirmed.is_verdict());
        assert!(MappingConfidence::Deterministic.is_verdict());
        assert!(!MappingConfidence::Unconfirmed.is_verdict());
        assert!(!MappingConfidence::Ambiguous.is_verdict());
    }

    #[test]
    fn an_unconfirmed_mapping_is_a_hint_and_nothing_more() {
        let hint = ExternalIdentity::unconfirmed(IdentityNamespace::Purl, "pkg:maven/a/b@1.0.0");
        assert!(!hint.is_verdict());
        assert_eq!(hint.confidence, MappingConfidence::Unconfirmed);
    }

    #[test]
    fn two_identities_of_different_namespaces_are_not_the_same_identity() {
        let digest = ExternalIdentity::confirmed(IdentityNamespace::OciDigest, "sha256:ab", "ev-1");
        let purl = ExternalIdentity::confirmed(IdentityNamespace::Purl, "sha256:ab", "ev-1");
        assert!(!digest.is_same_as(&purl));
    }
}
