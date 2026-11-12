//! The model of VEX, the statements that say what a vulnerability does not
//! affect.
//!
//! A VEX statement is a claim: "this vulnerability does not affect this
//! product, and here is why". Hodei takes the statement seriously, and it
//! also says how seriously: a statement that was **verified** is a fact
//! about the product, and a statement that nobody verified is a claim
//! that somebody wrote.
//!
//! The difference is the whole point. A VEX that was not verified does
//! not silence a finding: the finding stays, and the statement is
//! recorded next to it, so that a reader can see that somebody said so
//! and that nobody checked.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What a VEX statement declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VexStatus {
    /// The product is not affected.
    NotAffected,
    /// The product is affected.
    Affected,
    /// The product was fixed.
    Fixed,
    /// Nobody knows yet.
    UnderInvestigation,
}

impl VexStatus {
    /// The name of the status.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotAffected => "not_affected",
            Self::Affected => "affected",
            Self::Fixed => "fixed",
            Self::UnderInvestigation => "under_investigation",
        }
    }
}

/// Why a statement says what it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VexJustification {
    /// The code is not there.
    ComponentNotPresent,
    /// The code is there but the vulnerable path is not.
    VulnerableCodeNotPresent,
    /// The code is there and the vulnerable path is not reachable.
    VulnerableCodeNotReachable,
    /// The code is there and the vulnerable path is reachable.
    VulnerableCodeReachable,
    /// The vulnerability was mitigated another way.
    InlineMitigationsAlreadyExist,
}

/// A VEX statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VexStatement {
    /// The identifier of the statement.
    ///
    /// Not every producer writes one, and a statement without an
    /// identifier is still a statement: the reader gives it the name it
    /// was asked for.
    #[serde(default)]
    pub id: String,
    /// The vulnerability the statement is about.
    ///
    /// OpenVEX writes it as an object with a name and other producers
    /// write it as a plain string. Both are read.
    #[serde(deserialize_with = "read_identifier")]
    pub vulnerability: String,
    /// The products the statement is about.
    ///
    /// OpenVEX writes a product as an object with an `@id`, and other
    /// producers write it as a plain string. Both are read, because the
    /// format is the same thing written two ways and not two formats.
    #[serde(deserialize_with = "read_products")]
    pub products: Vec<String>,
    /// What it declares.
    pub status: VexStatus,
    /// Why it declares it.
    #[serde(default)]
    pub justification: Option<VexJustification>,
    /// The moment the statement was made.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    /// Whether the statement was verified.
    ///
    /// A statement that nobody verified is a claim, and it does not
    /// silence a finding on its own.
    #[serde(default)]
    pub verified: bool,
}

impl VexStatement {
    /// A statement that does not affect a component.
    pub fn not_affected(
        id: impl Into<String>,
        component: impl Into<String>,
        status: VexStatus,
        verified: bool,
    ) -> Self {
        Self {
            id: id.into(),
            vulnerability: String::new(),
            products: vec![component.into()],
            status,
            justification: None,
            timestamp: None,
            verified,
        }
    }

    /// Whether the statement is about a component.
    ///
    /// The comparison understands the two ways the same package is
    /// written: a VEX names the product by its PURL and the finding
    /// names it by its coordinates. They are the same package, and a
    /// statement that is not recognised is a statement that does not
    /// apply -- which is the safe direction, because the finding stays.
    pub fn does_not_apply_to(&self, component: &str) -> bool {
        !self
            .products
            .iter()
            .any(|product| product_matches(product, component))
    }

    /// Whether the statement was verified.
    pub fn is_verified(&self) -> bool {
        self.verified
    }
}

/// Reads a VEX statement from an OpenVEX document.
///
/// The document that OpenVEX defines has the statements under `statements`.
/// Hodei reads the one that carries the identifier it is asked for.
pub fn read_vex(document: &str, id: &str) -> Result<VexStatement, VexError> {
    let parsed: serde_json::Value = serde_json::from_str(document).map_err(|error| VexError {
        reason: format!("the document is not a VEX document: {error}"),
    })?;

    let statements = parsed
        .get("statements")
        .and_then(|value| value.as_array())
        .ok_or_else(|| VexError {
            reason: "the document has no statements".to_string(),
        })?;

    let statement = statements
        .iter()
        .find(|statement| {
            statement
                .get("vulnerability")
                .and_then(|value| match value {
                    serde_json::Value::String(text) => Some(text.as_str()),
                    other => other.get("name").and_then(|name| name.as_str()),
                })
                .is_some_and(|vulnerability| vulnerability == id)
                || statement
                    .get("@id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|identifier| identifier == id)
        })
        .ok_or_else(|| VexError {
            reason: format!("the document has no statement about {id}"),
        })?;

    let mut statement: VexStatement =
        serde_json::from_value(statement.clone()).map_err(|error| VexError {
            reason: format!("the statement is not a VEX statement: {error}"),
        })?;
    if statement.id.is_empty() {
        statement.id = id.to_string();
    }
    Ok(statement)
}

/// Whether a product of a statement is a component.
fn product_matches(product: &str, component: &str) -> bool {
    if product == component {
        return true;
    }

    // A PURL names the package inside it: `pkg:maven/org.example/lib-b@2.0.0`
    // names `org.example:lib-b`.
    let Some(rest) = product.strip_prefix("pkg:") else {
        return false;
    };
    let without_type = rest.split_once('/').map(|(_, name)| name).unwrap_or(rest);
    let name = without_type
        .split_once('@')
        .map(|(name, _)| name)
        .unwrap_or(without_type);
    let name = name.split_once('?').map(|(name, _)| name).unwrap_or(name);

    name.replace('/', ":") == component || name == component
}

/// Reads an identifier, written either as a string or as an object with a
/// name.
fn read_identifier<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(match &raw {
        serde_json::Value::String(text) => text.clone(),
        other => other
            .get("name")
            .and_then(|name| name.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
    })
}

/// Reads the products of a statement, written either way.
fn read_products<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(text) => text,
            other => other
                .get("@id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
                .unwrap_or_default(),
        })
        .collect())
}

/// Why a VEX document could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VexError {
    /// What is wrong with it.
    pub reason: String,
}

impl std::fmt::Display for VexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for VexError {}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"{
      "@context": "https://openvex.dev/ns/v0.2.0",
      "statements": [
        {
          "vulnerability": {"name": "CVE-2024-0001"},
          "products": [{"@id": "pkg:maven/org.example/lib-b@2.0.0"}],
          "status": "not_affected",
          "justification": "vulnerable_code_not_present"
        }
      ]
    }"#;

    #[test]
    fn a_statement_is_read_by_its_vulnerability() {
        let statement = read_vex(DOCUMENT, "CVE-2024-0001").expect("the document is read");
        assert_eq!(statement.status, VexStatus::NotAffected);
        assert_eq!(
            statement.justification,
            Some(VexJustification::VulnerableCodeNotPresent)
        );
    }

    #[test]
    fn a_document_without_the_statement_is_an_error() {
        let error = read_vex(DOCUMENT, "CVE-9999-9999").expect_err("it is not there");
        assert!(error.reason.contains("no statement about"));
    }
}
