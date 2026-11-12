//! The normalizers that read a document of inventory into Hodei's facts.
//!
//! Two standards describe the same world: CycloneDX 1.7 and SPDX 3.0. Both
//! are read into the **same native model**, and that is what the
//! compatibility path is: not a second model, and not a second wire format,
//! but a second normalizer that produces the same facts.
//!
//! # What a normalizer does and does not do
//!
//! A normalizer reads a document and produces facts. It does not decide
//! whether the document is true, and it does not merge the document with
//! another one. The document travels beside the facts, under its own
//! digest, and is never rewritten.
//!
//! # The version of the normalizer
//!
//! The version is part of the facts. A different version of the normalizer
//! is a different reading of the same document, and it takes part in the
//! idempotency key of the evidence: the same document read by two versions
//! is two pieces of evidence, not one.

use crate::domain::content::ContentDigest;
use crate::domain::standards::identity::{
    CANONICALIZATION_VERSION, ExternalIdentity, IdentityNamespace, MappingConfidence,
};
use crate::domain::standards::sbom::{
    ComponentKind, Sbom, SbomComponent, SbomDependency, SbomFormat, SbomSubject,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// The version of the CycloneDX normalizer.
pub const CYCLONEDX_NORMALIZER_VERSION: &str = "1.0.0";

/// The version of the SPDX normalizer.
pub const SPDX_NORMALIZER_VERSION: &str = "1.0.0";

/// Why a document of inventory could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    /// The document is not JSON.
    #[error("the document is not JSON: {0}")]
    MalformedJson(String),
    /// The document does not declare the format it is in.
    #[error("the document does not declare a format this version reads: {0}")]
    UnknownFormat(String),
    /// The document declares a version this normalizer does not read.
    #[error("the document declares the version {found} and this normalizer reads {expected}")]
    UnsupportedVersion {
        /// The version the document declares.
        found: String,
        /// The version the normalizer reads.
        expected: String,
    },
    /// The document has no subject: it says nothing about what it is about.
    #[error("the document does not say what it is about")]
    NoSubject,
}

/// Reads a CycloneDX 1.7 document into the native model.
///
/// The normalizer reads the facts the product asks about and leaves the
/// rest in the document: the tools that produced it, the licences, the
/// properties of a component that are not identities.
pub fn read_cyclonedx_17(document: &str) -> Result<Sbom, InventoryError> {
    let value: Value = serde_json::from_str(document)
        .map_err(|error| InventoryError::MalformedJson(error.to_string()))?;

    if value.get("bomFormat").and_then(Value::as_str) != Some("CycloneDX") {
        return Err(InventoryError::UnknownFormat(
            value
                .get("bomFormat")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_string(),
        ));
    }
    let spec_version = value
        .get("specVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if spec_version != "1.7" {
        return Err(InventoryError::UnsupportedVersion {
            found: spec_version.to_string(),
            expected: "1.7".to_string(),
        });
    }

    let subject = value
        .get("metadata")
        .and_then(|metadata| metadata.get("component"))
        .map(component_of)
        .transpose()?
        .map(|component| SbomSubject {
            reference: component.reference,
            name: component.name,
            version: component.version,
            kind: component.kind,
            identities: component.identities,
        })
        .ok_or(InventoryError::NoSubject)?;

    let components = value
        .get("components")
        .and_then(Value::as_array)
        .map(|components| {
            components
                .iter()
                .map(component_of)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();

    // The subject is a component of the inventory: it is the thing the
    // document is about, and the dependencies of the document start at it.
    // A document that names it only as the subject still has a component
    // there, and the native model has to have it for the edges to reach
    // it.
    let mut components = components;
    if !components
        .iter()
        .any(|component| component.reference == subject.reference)
    {
        components.push(SbomComponent {
            reference: subject.reference.clone(),
            name: subject.name.clone(),
            version: subject.version.clone(),
            kind: subject.kind.clone(),
            identities: subject.identities.clone(),
        });
    }

    let dependencies = value
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .map(|dependency| SbomDependency {
                    reference: dependency
                        .get("ref")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    depends_on: dependency
                        .get("dependsOn")
                        .and_then(Value::as_array)
                        .map(|refs| {
                            refs.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                // CycloneDX writes an entry for every component, and an
                // entry with nothing in it says that the component depends
                // on nothing. The native model records the edges that
                // exist: a component that is not the start of an edge
                // depends on nothing, and that is already said by its
                // absence.
                .filter(|dependency| !dependency.depends_on.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(Sbom {
        format: SbomFormat::CycloneDx17,
        normalizer_version: CYCLONEDX_NORMALIZER_VERSION.to_string(),
        subject: Some(subject),
        components,
        dependencies,
        source_digest: ContentDigest::of_bytes(document.as_bytes()),
        document_timestamp: value
            .get("metadata")
            .and_then(|metadata| metadata.get("timestamp"))
            .and_then(Value::as_str)
            .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
            .map(|timestamp| timestamp.with_timezone(&chrono::Utc)),
    })
}

/// Reads an SPDX 3.0 document into the native model.
///
/// The document names the same things the CycloneDX document names, in
/// another vocabulary: the packages are the components, the relationships
/// are the dependencies, and the external references of a package are its
/// identities. The facts that come out are the same facts.
pub fn read_spdx_30(document: &str) -> Result<Sbom, InventoryError> {
    let value: Value = serde_json::from_str(document)
        .map_err(|error| InventoryError::MalformedJson(error.to_string()))?;

    let spdx_version = value
        .get("spdxVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !spdx_version.starts_with("SPDX-3.") {
        return Err(InventoryError::UnknownFormat(spdx_version.to_string()));
    }
    if spdx_version != "SPDX-3.0" {
        return Err(InventoryError::UnsupportedVersion {
            found: spdx_version.to_string(),
            expected: "3.0".to_string(),
        });
    }

    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut components = Vec::with_capacity(packages.len());
    let mut by_spdx_id: BTreeMap<String, String> = BTreeMap::new();

    for package in &packages {
        let spdx_id = package
            .get("SPDXID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let version = package
            .get("versionInfo")
            .and_then(Value::as_str)
            .map(str::to_string);

        // The reference of the native model is the identifier of the
        // document, so that the relationships point at the same thing the
        // components do.
        let reference = spdx_id.clone();
        by_spdx_id.insert(spdx_id, reference.clone());

        components.push(SbomComponent {
            reference,
            name,
            version,
            kind: ComponentKind::Library,
            identities: identities_of_spdx(package),
        });
    }

    // The subject is the package the document describes: the one the
    // document itself points at with a DESCRIBES relation, or, when there
    // is none, the first package that has a version.
    let subject = subject_of_spdx(&value, &packages).ok_or(InventoryError::NoSubject)?;

    // The subject is a component of the inventory, the same rule the
    // CycloneDX reader applies.
    if !components
        .iter()
        .any(|component| component.reference == subject.reference)
    {
        components.push(SbomComponent {
            reference: subject.reference.clone(),
            name: subject.name.clone(),
            version: subject.version.clone(),
            kind: subject.kind.clone(),
            identities: subject.identities.clone(),
        });
    }

    let dependencies = value
        .get("relationships")
        .and_then(Value::as_array)
        .map(|relationships| {
            let mut dependencies: Vec<SbomDependency> = Vec::new();
            for relationship in relationships {
                let kind = relationship
                    .get("relationshipType")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if kind != "DEPENDS_ON" {
                    continue;
                }
                let (from, to) = sides_of_relationship(relationship);
                let from = by_spdx_id.get(&from).cloned().unwrap_or(from);
                let to = by_spdx_id.get(&to).cloned().unwrap_or(to);
                match dependencies
                    .iter_mut()
                    .find(|dependency| dependency.reference == from)
                {
                    Some(dependency) => dependency.depends_on.push(to),
                    None => dependencies.push(SbomDependency {
                        reference: from,
                        depends_on: vec![to],
                    }),
                }
            }
            dependencies
        })
        .unwrap_or_default();

    Ok(Sbom {
        format: SbomFormat::Spdx30,
        normalizer_version: SPDX_NORMALIZER_VERSION.to_string(),
        subject: Some(subject),
        components,
        dependencies,
        source_digest: ContentDigest::of_bytes(document.as_bytes()),
        document_timestamp: value
            .get("creationInfo")
            .and_then(|creation| creation.get("created"))
            .and_then(Value::as_str)
            .and_then(|created| chrono::DateTime::parse_from_rfc3339(created).ok())
            .map(|created| created.with_timezone(&chrono::Utc)),
    })
}

/// The two sides of an SPDX relationship.
///
/// SPDX 3.0 names them `spdxElementId` and `relatedSpdxElement`, and
/// SPDX 2.x named them `from` and `to`. The reader takes the names of the
/// version it reads.
fn sides_of_relationship(relationship: &Value) -> (String, String) {
    let from = relationship
        .get("spdxElementId")
        .or_else(|| relationship.get("from"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let to = relationship
        .get("relatedSpdxElement")
        .or_else(|| relationship.get("to"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (from, to)
}

/// The subject of an SPDX document: the package it describes.
fn subject_of_spdx(value: &Value, packages: &[Value]) -> Option<SbomSubject> {
    let described: Vec<String> = value
        .get("relationships")
        .and_then(Value::as_array)
        .map(|relationships| {
            relationships
                .iter()
                .filter(|relationship| {
                    relationship.get("relationshipType").and_then(Value::as_str)
                        == Some("DESCRIBES")
                })
                .map(|relationship| sides_of_relationship(relationship).1)
                .collect()
        })
        .unwrap_or_default();

    let subject_package = packages.iter().find(|package| {
        package
            .get("SPDXID")
            .and_then(Value::as_str)
            .is_some_and(|id| described.iter().any(|described| described == id))
    })?;

    Some(SbomSubject {
        reference: subject_package
            .get("SPDXID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: subject_package
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        version: subject_package
            .get("versionInfo")
            .and_then(Value::as_str)
            .map(str::to_string),
        kind: ComponentKind::Library,
        identities: identities_of_spdx(subject_package),
    })
}

/// The identities of an SPDX package, read from its external references.
fn identities_of_spdx(package: &Value) -> Vec<ExternalIdentity> {
    let mut identities = Vec::new();

    for reference in package
        .get("externalRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let reference_type = reference
            .get("referenceType")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let locator = reference
            .get("referenceLocator")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if reference_type.eq_ignore_ascii_case("purl") {
            identities.push(ExternalIdentity {
                namespace: IdentityNamespace::Purl,
                value: locator.to_string(),
                // The document says the package has this PURL: it is an
                // assertion of the document, not an inference.
                confidence: MappingConfidence::Confirmed,
                source_evidence: None,
                canonicalization_version: CANONICALIZATION_VERSION.to_string(),
                valid_from: None,
                valid_to: None,
            });
        } else if reference_type.eq_ignore_ascii_case("sha256") {
            identities.push(ExternalIdentity {
                namespace: IdentityNamespace::ContentDigest,
                value: format!("sha256:{locator}"),
                confidence: MappingConfidence::Confirmed,
                source_evidence: None,
                canonicalization_version: CANONICALIZATION_VERSION.to_string(),
                valid_from: None,
                valid_to: None,
            });
        }
    }

    identities
}

/// The name of a digest algorithm, as Hodei names it.
///
/// The standards write it in their own way: CycloneDX says `SHA-256` and
/// SPDX says `sha256`. The native model names it once, so that the two
/// documents say the same thing about the same bytes.
fn normalize_algorithm(algorithm: &str) -> String {
    match algorithm.to_ascii_lowercase().replace('-', "").as_str() {
        "sha256" => "sha256".to_string(),
        "sha512" => "sha512".to_string(),
        "sha1" => "sha1".to_string(),
        "md5" => "md5".to_string(),
        other => other.to_string(),
    }
}

/// Reads a CycloneDX component, with the identities the document gives it.
fn component_of(value: &Value) -> Result<SbomComponent, InventoryError> {
    let reference = value
        .get("bom-ref")
        .and_then(Value::as_str)
        .or_else(|| value.get("purl").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    let kind = match value.get("type").and_then(Value::as_str) {
        Some("library") => ComponentKind::Library,
        Some("application") => ComponentKind::Application,
        Some("container") => ComponentKind::Container,
        Some("framework") => ComponentKind::Framework,
        Some("operating-system") => ComponentKind::OperatingSystem,
        Some("device") => ComponentKind::Device,
        Some("file") => ComponentKind::File,
        Some(other) => ComponentKind::Other(other.to_string()),
        None => ComponentKind::Other("unknown".to_string()),
    };

    let mut identities = Vec::new();

    if let Some(purl) = value.get("purl").and_then(Value::as_str) {
        identities.push(ExternalIdentity {
            namespace: IdentityNamespace::Purl,
            value: purl.to_string(),
            // The document says the component has this PURL.
            confidence: MappingConfidence::Confirmed,
            source_evidence: None,
            canonicalization_version: CANONICALIZATION_VERSION.to_string(),
            valid_from: None,
            valid_to: None,
        });
    }

    for hash in value
        .get("hashes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let algorithm =
            normalize_algorithm(hash.get("alg").and_then(Value::as_str).unwrap_or_default());
        let content = hash
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let namespace = match algorithm.as_str() {
            "sha256" | "sha512" => IdentityNamespace::ContentDigest,
            _ => IdentityNamespace::Other(format!("digest_{algorithm}")),
        };
        identities.push(ExternalIdentity {
            namespace,
            value: format!("{algorithm}:{content}"),
            confidence: MappingConfidence::Confirmed,
            source_evidence: None,
            canonicalization_version: CANONICALIZATION_VERSION.to_string(),
            valid_from: None,
            valid_to: None,
        });
    }

    Ok(SbomComponent {
        reference,
        name,
        version,
        kind,
        identities,
    })
}
