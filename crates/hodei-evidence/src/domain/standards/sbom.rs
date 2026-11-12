//! The native model of an SBOM: Hodei's own shape, not the standard's.
//!
//! The reference standards say it plainly: "preserve source document and
//! normalize only product-relevant facts". So this is not a CycloneDX
//! document in Rust types, and it is not an SPDX document either. It is the
//! set of facts Hodei can answer product questions with, and the document
//! it came from travels beside it, untouched.
//!
//! # Why the model is not the standard's
//!
//! Two standards describe the same world and they do not agree on their
//! vocabulary. If the model were the standard's, every question would have
//! to ask which standard it is speaking about, and a policy that wants "the
//! components of this artifact" would need two implementations. With a
//! native model, the two standards are **normalizers**, and the facts they
//! produce are the same facts.
//!
//! # What is product-relevant
//!
//! - **What the thing is**: its name, its version, its PURL, its digest.
//! - **What is inside it**: the components and how they depend on each
//!   other.
//! - **What it is about**: the subject the document describes.
//!
//! What is not product-relevant stays in the raw document: the tools that
//! produced it, the licences, the vulnerabilities of a document that is an
//! inventory and not a risk report.

use crate::domain::content::ContentDigest;
use crate::domain::standards::identity::ExternalIdentity;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The format a document of inventory was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SbomFormat {
    /// CycloneDX 1.7.
    CycloneDx17,
    /// SPDX 3.0.
    Spdx30,
}

impl SbomFormat {
    /// The name of the normalizer that reads this format.
    pub fn normalizer(&self) -> &'static str {
        match self {
            Self::CycloneDx17 => "cyclonedx",
            Self::Spdx30 => "spdx",
        }
    }

    /// The version of the format itself.
    pub fn format_version(&self) -> &'static str {
        match self {
            Self::CycloneDx17 => "1.7",
            Self::Spdx30 => "3.0",
        }
    }
}

impl fmt::Display for SbomFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CycloneDx17 => f.write_str("cyclonedx-1.7"),
            Self::Spdx30 => f.write_str("spdx-3.0"),
        }
    }
}

/// The kind of a component of an inventory.
///
/// The kinds are the ones the domain model of the plan names. A kind that
/// this version does not know keeps its name instead of being forced into
/// one of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    /// A library.
    Library,
    /// An application.
    Application,
    /// A container image.
    Container,
    /// A framework.
    Framework,
    /// An operating system.
    OperatingSystem,
    /// A device.
    Device,
    /// A file.
    File,
    /// A kind that this version of Hodei does not know.
    #[serde(untagged)]
    Other(String),
}

impl fmt::Display for ComponentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Library => f.write_str("library"),
            Self::Application => f.write_str("application"),
            Self::Container => f.write_str("container"),
            Self::Framework => f.write_str("framework"),
            Self::OperatingSystem => f.write_str("operating_system"),
            Self::Device => f.write_str("device"),
            Self::File => f.write_str("file"),
            Self::Other(value) => f.write_str(value),
        }
    }
}

/// A component of an inventory.
///
/// A component carries the identities the document gave it, and nothing
/// else: the names are what the world calls it, and the confidence says how
/// sure Hodei is that two names are the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbomComponent {
    /// The key the document uses to refer to this component.
    ///
    /// It is the reference of the document, and it is what the
    /// dependencies point at. It is not an identity of the component: two
    /// documents may call the same component different things.
    pub reference: String,
    /// The name the document gives it.
    pub name: String,
    /// The version the document gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// What kind of thing it is.
    pub kind: ComponentKind,
    /// The identities the document gave it.
    #[serde(default)]
    pub identities: Vec<ExternalIdentity>,
}

impl SbomComponent {
    /// The identity of this component in a namespace, when it has one.
    pub fn identity_in(
        &self,
        namespace: &crate::domain::standards::identity::IdentityNamespace,
    ) -> Option<&ExternalIdentity> {
        self.identities
            .iter()
            .find(|identity| &identity.namespace == namespace)
    }

    /// The PURL of the component, when it has one.
    pub fn purl(&self) -> Option<&str> {
        self.identity_in(&crate::domain::standards::identity::IdentityNamespace::Purl)
            .map(|identity| identity.value.as_str())
    }

    /// The digest of the component, when the document gave one.
    ///
    /// The digest is the identity of the bytes, and the document names it
    /// in the namespace of the algorithm that produced it.
    pub fn digest(&self) -> Option<ContentDigest> {
        self.identities
            .iter()
            .filter(|identity| {
                identity.namespace
                    == crate::domain::standards::identity::IdentityNamespace::ContentDigest
            })
            .find_map(|identity| {
                let (algorithm, value) = identity.value.split_once(':')?;
                match algorithm {
                    "sha256"
                        if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) =>
                    {
                        Some(ContentDigest::sha256(value))
                    }
                    _ => None,
                }
            })
    }
}

/// What a component of an inventory depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbomDependency {
    /// The reference of the component that depends.
    pub reference: String,
    /// The references of the components it depends on.
    pub depends_on: Vec<String>,
}

/// The subject an inventory is about.
///
/// The subject is what the document says the inventory describes. It is the
/// thing whose trust the document is evidence about, and it is the thing
/// that a later question is asked about.
///
/// The subject is also a **component** of the inventory: the dependencies
/// of the document start at it, and the native model has to be able to
/// reach it by its reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbomSubject {
    /// The key the document uses to refer to it.
    pub reference: String,
    /// The name the document gives it.
    pub name: String,
    /// The version the document gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// What kind of thing it is.
    pub kind: ComponentKind,
    /// The identities the document gave it.
    #[serde(default)]
    pub identities: Vec<ExternalIdentity>,
}

/// The native model of an inventory.
///
/// The model is the facts. The document that carried them travels beside it
/// in the content store, under its own digest, and is never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sbom {
    /// The format the document was written in.
    pub format: SbomFormat,
    /// The version of the normalizer that produced these facts.
    pub normalizer_version: String,
    /// What the inventory is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<SbomSubject>,
    /// The components of the inventory.
    pub components: Vec<SbomComponent>,
    /// What depends on what.
    #[serde(default)]
    pub dependencies: Vec<SbomDependency>,
    /// The digest of the document the facts came from.
    pub source_digest: ContentDigest,
    /// When the document says the inventory was made.
    ///
    /// The measure of the standards (SPIKE-005) found this missing: a
    /// document that says when it was made carries a fact that the
    /// freshness of the evidence needs, and a normalizer that drops it
    /// loses it for good. The fact is the document's, and it is kept apart
    /// from the moment Hodei learned of it, which is a different moment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_timestamp: Option<DateTime<Utc>>,
}

impl Sbom {
    /// The component the document refers to by a reference.
    pub fn component(&self, reference: &str) -> Option<&SbomComponent> {
        self.components
            .iter()
            .find(|component| component.reference == reference)
    }

    /// What a component depends on, by reference.
    pub fn dependencies_of(&self, reference: &str) -> Vec<String> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.reference == reference)
            .map(|dependency| dependency.depends_on.clone())
            .unwrap_or_default()
    }

    /// Every component the given one reaches, directly or through others.
    ///
    /// The walk follows the dependencies of the document and does not guess
    /// an order: a dependency that the document does not say is not
    /// traversed.
    pub fn transitive_dependencies(&self, reference: &str) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut pending: Vec<String> = self.dependencies_of(reference);

        while let Some(current) = pending.pop() {
            if seen.contains(&current) {
                continue;
            }
            seen.push(current.clone());
            for next in self.dependencies_of(&current) {
                if !seen.contains(&next) {
                    pending.push(next);
                }
            }
        }
        seen
    }
}
