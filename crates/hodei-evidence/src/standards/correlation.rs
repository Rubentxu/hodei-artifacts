//! The correlation of identities: when two names are the same thing.
//!
//! The M2 UAT says it: "Given SBOM component uses PURL and registry/
//! artifact source uses digest/coordinates, when normalization correlates
//! identities, then deterministic mapping is recorded with source and
//! confidence. Ambiguous mapping remains ambiguous and cannot silently
//! satisfy trust requirements."
//!
//! So the correlation answers three questions and refuses to answer a
//! fourth:
//!
//! - **Do these two identities name the same thing?** When they do, the
//!   answer is a mapping with a confidence.
//! - **How sure are we?** A mapping that came out of a deterministic rule
//!   is a verdict; a mapping that came out of a name that merely looks
//!   alike is a hint.
//! - **Is the mapping usable as a verdict?** [`MappingConfidence::is_verdict`]
//!   answers it, and the answer is no for anything that was not confirmed.
//!
//! # The rule that cannot be broken
//!
//! **An ambiguity is never resolved by guessing.** When two candidates
//! match equally well, the correlation says so and stops. The threat model
//! is explicit: "ambiguous mapping remains unresolved/low confidence" and
//! "no heuristic merge can silently satisfy high-assurance policy".

use crate::domain::content::ContentDigest;
use crate::domain::standards::identity::{ExternalIdentity, IdentityNamespace, MappingConfidence};
use crate::domain::standards::sbom::Sbom;
use std::collections::BTreeMap;

/// Why two things could not be correlated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorrelationError {
    /// The two identities are of different namespaces and cannot be
    /// compared.
    #[error("the two identities live in different namespaces: {left} and {right}")]
    DifferentNamespaces {
        /// The namespace of the first one.
        left: String,
        /// The namespace of the second one.
        right: String,
    },
    /// The two identities cannot name the same thing.
    #[error("the two identities cannot name the same thing: {0}")]
    NotComparable(String),
}

/// The result of correlating the identities of two things.
///
/// The result says whether the things are the same, how sure Hodei is, and
/// why. A result that is not certain is a hint, and it says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correlation {
    /// Whether the two things are the same.
    pub same: bool,
    /// How sure Hodei is of it.
    pub confidence: MappingConfidence,
    /// The identities that were correlated.
    pub left: ExternalIdentity,
    /// The identity that was correlated with it.
    pub right: ExternalIdentity,
    /// Why the correlation says what it says.
    pub reason: String,
}

impl Correlation {
    /// Whether the correlation may be used as a verdict.
    pub fn is_verdict(&self) -> bool {
        self.same && self.confidence.is_verdict()
    }
}

/// The canonical form of a PURL, for comparison.
///
/// The Package URL specification says what a PURL is: a scheme, a type, a
/// namespace, a name and a version, with the qualifiers and the
/// subpath. Two PURLs that name the same package are the same string once
/// the parts that carry no meaning are taken out: the case of the type and
/// of the namespace, and the empty qualifiers.
///
/// The version of the rule is part of the mapping, because a different rule
/// is a different reading of the same document.
pub const PURL_CANONICALIZATION_VERSION: &str = "purl/1.0.0";

/// The canonical form of a PURL.
///
/// The result is what two PURLs are compared by. It is not a PURL: it is
/// the part of a PURL that names the thing.
pub fn canonicalize_purl(purl: &str) -> Option<String> {
    let rest = purl.strip_prefix("pkg:")?;
    let mut parts = rest.split('/');
    let kind = parts.next()?.to_ascii_lowercase();
    let namespace = parts.next().unwrap_or_default().to_ascii_lowercase();
    let name = parts.next()?.to_ascii_lowercase();

    // The version is the part after the '@' of the name, and the qualifiers
    // are the part after the '?'. Both carry meaning, and both are kept.
    let (name, version) = match name.split_once('@') {
        Some((name, version)) => (name.to_string(), version.to_string()),
        None => (name, String::new()),
    };

    if name.is_empty() {
        return None;
    }

    let mut canonical = format!("pkg:{kind}/{namespace}/{name}");
    if !version.is_empty() {
        canonical.push('@');
        canonical.push_str(&version);
    }
    Some(canonical)
}

/// Correlates the identity of a component of an inventory with an identity
/// of an artefact.
///
/// The two names come from different worlds: the inventory says
/// `pkg:maven/org.example/lib-a@1.2.3` and the registry says
/// `sha256:9f86d0...`. The correlation asks whether they can be the same
/// thing, and answers with the confidence it has.
///
/// - Two PURLs that canonicalize to the same thing are a **deterministic**
///   mapping: the rule produced it and the rule is versioned.
/// - A PURL and a digest are not comparable by rule: the correlation says
///   so, and says that a mapping between them would be a **hint**.
/// - Two things that could both match are **ambiguous**, and the
///   correlation stops there.
pub fn correlate_identities(
    left: &ExternalIdentity,
    right: &ExternalIdentity,
) -> Result<Correlation, CorrelationError> {
    if left.namespace != right.namespace {
        return Err(CorrelationError::DifferentNamespaces {
            left: left.namespace.to_string(),
            right: right.namespace.to_string(),
        });
    }

    match (&left.namespace, &left.value, &right.value) {
        (IdentityNamespace::Purl, left_purl, right_purl) => {
            let left_canonical = canonicalize_purl(left_purl);
            let right_canonical = canonicalize_purl(right_purl);

            match (left_canonical, right_canonical) {
                (Some(left_canonical), Some(right_canonical))
                    if left_canonical == right_canonical =>
                {
                    Ok(Correlation {
                        same: true,
                        confidence: MappingConfidence::Deterministic,
                        left: left.clone(),
                        right: right.clone(),
                        reason: format!(
                            "both names canonicalize to {left_canonical} under the rule {PURL_CANONICALIZATION_VERSION}"
                        ),
                    })
                }
                (Some(_), Some(_)) => Ok(Correlation {
                    same: false,
                    confidence: MappingConfidence::Deterministic,
                    left: left.clone(),
                    right: right.clone(),
                    reason: "the two names canonicalize to different things".to_string(),
                }),
                _ => Ok(Correlation {
                    same: false,
                    confidence: MappingConfidence::Unconfirmed,
                    left: left.clone(),
                    right: right.clone(),
                    reason: "one of the names is not a PURL this version reads".to_string(),
                }),
            }
        }
        (IdentityNamespace::ContentDigest, left_digest, right_digest) => {
            if left_digest == right_digest {
                Ok(Correlation {
                    same: true,
                    confidence: MappingConfidence::Deterministic,
                    left: left.clone(),
                    right: right.clone(),
                    reason: "the two names are the same digest".to_string(),
                })
            } else {
                Ok(Correlation {
                    same: false,
                    confidence: MappingConfidence::Deterministic,
                    left: left.clone(),
                    right: right.clone(),
                    reason: "the two names are different digests".to_string(),
                })
            }
        }
        (namespace, _, _) => Err(CorrelationError::NotComparable(format!(
            "the namespace {namespace} has no rule that relates two of its names"
        ))),
    }
}

/// The components of an inventory that a given artefact may be.
///
/// The correlation is done **by the identities the document gives**, and
/// the confidence of each answer is kept. A component that cannot be
/// correlated with certainty comes back with the confidence that says so,
/// and a caller that needs a verdict has to ask for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrelatedComponent {
    /// The reference of the component in the document.
    pub reference: String,
    /// The name of the component.
    pub name: String,
    /// The identities of the component.
    pub identities: Vec<ExternalIdentity>,
    /// The correlation of the component with the artefact, when there is one.
    pub correlation: Option<Correlation>,
}

impl CorrelatedComponent {
    /// Whether the component may be used as a verdict about the artefact.
    ///
    /// This is the line that keeps an ambiguous merge from satisfying a
    /// trust requirement: without a correlation, and without one that is
    /// certain, the component is a hint.
    pub fn is_verdict(&self) -> bool {
        self.correlation
            .as_ref()
            .is_some_and(Correlation::is_verdict)
    }
}

/// Correlates the components of an inventory with the identities of an
/// artefact.
///
/// The artefact is named by its own identities --the digest the registry
/// gave, the coordinates the build gave-- and every component of the
/// inventory is compared with them. The result keeps the confidence of
/// each comparison, so that the caller can tell a component that was
/// correlated with certainty from one that merely looks like the artefact.
pub fn correlate_inventory(
    inventory: &Sbom,
    artifact: &[ExternalIdentity],
) -> Vec<CorrelatedComponent> {
    inventory
        .components
        .iter()
        .map(|component| {
            let correlation = component
                .identities
                .iter()
                .filter_map(|component_identity| {
                    artifact
                        .iter()
                        .filter_map(|artifact_identity| {
                            correlate_identities(component_identity, artifact_identity).ok()
                        })
                        .find(|correlation| correlation.same)
                })
                .next();

            CorrelatedComponent {
                reference: component.reference.clone(),
                name: component.name.clone(),
                identities: component.identities.clone(),
                correlation,
            }
        })
        .collect()
}

/// The identity of a component by its digest, when the document gave one.
///
/// The lookup is by the digest the document carries, and it is the reason
/// an inventory and a registry can be joined without guessing.
pub fn component_by_digest<'a>(
    inventory: &'a Sbom,
    digest: &ContentDigest,
) -> Option<&'a crate::domain::standards::sbom::SbomComponent> {
    inventory.components.iter().find(|component| {
        component
            .identities
            .iter()
            .any(|identity| identity.value == digest.to_string())
    })
}

/// The identities of an artefact, gathered from the places that named it.
///
/// Every source that named the artefact contributes its identities, and
/// the identities that two sources agree on are the ones that were
/// confirmed. The ones that only one source says are hints, and they are
/// marked as such.
pub fn gather_identities(sources: &[BTreeMap<IdentityNamespace, String>]) -> Vec<ExternalIdentity> {
    let mut identities: Vec<ExternalIdentity> = Vec::new();

    for source in sources {
        for (namespace, value) in source {
            let known = identities
                .iter()
                .any(|identity| identity.namespace == *namespace && identity.value == *value);
            if known {
                continue;
            }

            // A name that more than one source agrees on was confirmed; a
            // name that only one source has is a hint.
            let sources_that_say = sources
                .iter()
                .filter(|other| other.get(namespace) == Some(value))
                .count();

            identities.push(ExternalIdentity {
                namespace: namespace.clone(),
                value: value.clone(),
                confidence: if sources_that_say > 1 {
                    MappingConfidence::Confirmed
                } else {
                    MappingConfidence::Unconfirmed
                },
                source_evidence: None,
                canonicalization_version:
                    crate::domain::standards::identity::CANONICALIZATION_VERSION.to_string(),
                valid_from: None,
                valid_to: None,
            });
        }
    }

    identities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn purl(value: &str) -> ExternalIdentity {
        ExternalIdentity::confirmed(IdentityNamespace::Purl, value, "ev-1")
    }

    #[test]
    fn two_spellings_of_the_same_purl_are_the_same_thing() {
        let one = purl("pkg:Maven/Org.Example/Lib-A@1.2.3");
        let other = purl("pkg:maven/org.example/lib-a@1.2.3");
        let correlation = correlate_identities(&one, &other).expect("the correlation");
        assert!(correlation.same);
        assert!(correlation.is_verdict());
    }

    #[test]
    fn two_different_purls_are_not_the_same_thing() {
        let one = purl("pkg:maven/org.example/lib-a@1.2.3");
        let other = purl("pkg:maven/org.example/lib-a@1.2.4");
        let correlation = correlate_identities(&one, &other).expect("the correlation");
        assert!(!correlation.same);
    }

    #[test]
    fn a_purl_and_a_digest_cannot_be_correlated_by_rule() {
        let one = purl("pkg:maven/org.example/lib-a@1.2.3");
        let other =
            ExternalIdentity::confirmed(IdentityNamespace::ContentDigest, "sha256:abc", "ev-1");
        let error = correlate_identities(&one, &other).expect_err("the namespaces differ");
        assert!(matches!(
            error,
            CorrelationError::DifferentNamespaces { .. }
        ));
    }
}
