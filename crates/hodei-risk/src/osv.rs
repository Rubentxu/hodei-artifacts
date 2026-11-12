//! The model of OSV, the format in which the vulnerability databases
//! publish.
//!
//! Hodei does not have a vulnerability database of its own. It reads what
//! the producers publish, in the format they publish it, and it says when
//! it does not know something.
//!
//! The model here is the subset of OSV that Hodei reads: the identity of
//! the advisory, the aliases it also answers to, the packages it affects
//! and the ranges of versions that are affected. Everything else in an
//! OSV document is carried in `database_specific` and `ecosystem_specific`
//! without being interpreted, because interpreting it would be guessing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The version of the OSV schema this model reads.
pub const OSV_SCHEMA_VERSION: &str = "1.6.0";

/// An advisory, as OSV publishes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvAdvisory {
    /// The schema the document was written for.
    #[serde(default)]
    pub schema_version: Option<String>,
    /// The identifier of the advisory, as its publisher gives it.
    pub id: String,
    /// The other identifiers the same problem has.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// When the advisory was published.
    #[serde(default)]
    pub published: Option<DateTime<Utc>>,
    /// When the advisory was last changed.
    #[serde(default)]
    pub modified: Option<DateTime<Utc>>,
    /// When the advisory was withdrawn.
    #[serde(default)]
    pub withdrawn: Option<DateTime<Utc>>,
    /// The packages the advisory is about.
    #[serde(default)]
    pub affected: Vec<OsvAffected>,
    /// The references of the advisory.
    #[serde(default)]
    pub references: Vec<OsvReference>,
    /// What the publisher says and that Hodei does not interpret.
    #[serde(default)]
    pub database_specific: serde_json::Value,
}

impl OsvAdvisory {
    /// Whether the advisory was withdrawn.
    ///
    /// A withdrawn advisory is not a vulnerability that is not there: it is
    /// a statement that the publisher withdrew the statement. Hodei keeps
    /// it and says that it was withdrawn.
    pub fn is_withdrawn(&self) -> bool {
        self.withdrawn.is_some()
    }

    /// The identifier the advisory also answers to, if it has one.
    pub fn cve(&self) -> Option<&str> {
        self.aliases
            .iter()
            .find(|alias| alias.starts_with("CVE-"))
            .map(String::as_str)
    }
}

/// A package the advisory is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvAffected {
    /// The package.
    pub package: OsvPackage,
    /// The ranges of versions that are affected.
    #[serde(default)]
    pub ranges: Vec<OsvRange>,
    /// The versions that are affected, one by one.
    #[serde(default)]
    pub versions: Vec<String>,
    /// What the ecosystem says and that Hodei does not interpret.
    #[serde(default)]
    pub ecosystem_specific: serde_json::Value,
}

impl OsvAffected {
    /// The version of the package, if the advisory names one.
    pub fn fixed_version(&self) -> Option<&str> {
        self.ranges
            .iter()
            .flat_map(|range| range.events.iter())
            .find_map(|event| event.fixed.as_deref())
    }
}

/// The identity of a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvPackage {
    /// The ecosystem the package belongs to.
    pub ecosystem: String,
    /// The name of the package.
    pub name: String,
    /// The PURL of the package, when the publisher gives one.
    #[serde(default)]
    pub purl: Option<String>,
}

/// A range of versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvRange {
    /// The kind of range: `ECOSYSTEM` or `GIT`.
    #[serde(rename = "type")]
    pub range_type: String,
    /// The events that describe the range.
    #[serde(default)]
    pub events: Vec<OsvEvent>,
}

/// One boundary of a range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvEvent {
    /// The version from which the range starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<String>,
    /// The version at which the range ends, and the problem is fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    /// The last version of the range, when the problem was never fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_affected: Option<String>,
    /// The limit of the range, which the Git ranges use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

/// A reference of an advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsvReference {
    /// The kind of reference.
    #[serde(rename = "type")]
    pub reference_type: String,
    /// Where it is.
    pub url: String,
}

/// Reads an advisory from an OSV document.
///
/// The document is read as it is. A document that does not have the shape
/// of an OSV document is an error and not an advisory with no content:
/// reading half an advisory and calling it an advisory would be how a
/// deployment ends up believing that a vulnerability was never published.
pub fn read_osv(document: &str) -> Result<OsvAdvisory, OsvError> {
    let advisory: OsvAdvisory = serde_json::from_str(document).map_err(|error| OsvError {
        reason: format!("the document is not an OSV advisory: {error}"),
    })?;
    if advisory.id.trim().is_empty() {
        return Err(OsvError {
            reason: "the advisory has no identifier".to_string(),
        });
    }
    Ok(advisory)
}

/// Why an OSV document could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsvError {
    /// What is wrong with it.
    pub reason: String,
}

impl std::fmt::Display for OsvError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for OsvError {}

#[cfg(test)]
mod tests {
    use super::*;

    const ADVISORY: &str = r#"{
      "schema_version": "1.6.0",
      "id": "GHSA-test-0001",
      "aliases": ["CVE-2024-0001"],
      "modified": "2024-05-01T00:00:00Z",
      "affected": [
        {
          "package": {
            "ecosystem": "Maven",
            "name": "org.example:lib-b",
            "purl": "pkg:maven/org.example/lib-b@2.0.0"
          },
          "ranges": [
            {
              "type": "ECOSYSTEM",
              "events": [{"introduced": "0"}, {"fixed": "2.0.1"}]
            }
          ]
        }
      ],
      "references": [{"type": "ADVISORY", "url": "https://example.test/1"}]
    }"#;

    #[test]
    fn an_advisory_is_read_as_it_is() {
        let advisory = read_osv(ADVISORY).expect("the document is read");
        assert_eq!(advisory.id, "GHSA-test-0001");
        assert_eq!(advisory.cve(), Some("CVE-2024-0001"));
        assert_eq!(advisory.affected.len(), 1);
        assert_eq!(advisory.affected[0].package.name, "org.example:lib-b");
        assert_eq!(advisory.affected[0].fixed_version(), Some("2.0.1"));
    }

    #[test]
    fn a_document_that_is_not_an_advisory_is_an_error() {
        let error = read_osv(r#"{"not":"an advisory"}"#).expect_err("it is not");
        assert!(error.reason.contains("not an OSV advisory"));
    }
}
