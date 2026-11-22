//! The licenses of the inventory: what the documents say, and nothing
//! beyond that.
//!
//! A license is a **fact about the inventory**, and Hodei records facts.
//! It does not interpret them: deciding whether two licenses can be
//! combined, or what a licence obliges, is legal advice, and an
//! automated system that gives it without a lawyer is a system that
//! guesses. So this module reads what the SPDX documents carry, says
//! what they do not carry, and stops there.
//!
//! The rules that keep the reading honest:
//!
//! - **An absent license expression stays absent.** The inventory that
//!   says nothing about a license is recorded as `NoAssertion` -- the
//!   SPDX term for "the document does not say" -- and it is never
//!   guessed at, filled with a default, or taken for a permissive one.
//!   An unknown license is not a risk and not a clean sheet either: it
//!   is an unknown, and the report says so.
//! - **A document written as an SPDX 2.3 expression is read as one
//!   expression and split into its tokens** -- `MIT OR Apache-2.0` is
//!   the two licenses `MIT` and `Apache-2.0` and a choice -- and the
//!   whole expression travels with them, because the meaning of the
//!   tokens depends on the expression.
//! - **Deprecated license identifiers are said out loud.** The SPDX
//!   list keeps old identifiers with a note; Hodei repeats the note and
//!   does not decide what it means for anybody.
//! - The list of identifiers Hodei knows is a snapshot of the SPDX
//!   license list. A license that is not in the snapshot is not
//!   unknown-by-definition: it is a license the document names, the
//!   snapshot does not carry, and the record says `Listed: false` so a
//!   reader can see which snapshot it was read against.

use serde::{Deserialize, Serialize};

/// The version of the SPDX license-list snapshot this module reads
/// against. The snapshot lives in the repository with the code, so that
/// a record says which list made it.
pub const SPDX_LICENSE_LIST_VERSION: &str = "3.25.0";

/// Where the license of a component came from, as the document wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseOrigin {
    /// The document gave an SPDX expression with identifiers in it.
    Expression,
    /// The document named a file that holds the license text.
    File,
    /// The document named the license only by a URL: the license has a
    /// place, not a name, and Hodei does not name it for them.
    Url,
    /// The document said nothing about the license, or said what SPDX
    /// says when it says nothing: `NOASSERTION`.
    Absent,
}

/// One license the inventory states about one component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseFact {
    /// The component the fact is about, as the inventory names it.
    pub component: String,
    /// The whole expression the document wrote, when it wrote one:
    /// `MIT OR Apache-2.0` travels as it came, because the meaning of
    /// each token depends on the operator that joins them.
    pub expression: Option<String>,
    /// The individual SPDX identifiers the expression is made of. They
    /// are the tokens of the expression, not a re-decision about what
    /// the component has.
    pub identifiers: Vec<String>,
    /// Whether each identifier is in the SPDX snapshot this module
    /// reads against. The boolean is per identifier and in the same
    /// order as `identifiers`.
    pub listed: Vec<bool>,
    /// Whether an identifier carries the deprecation note of the list.
    pub deprecated: Vec<bool>,
    /// Where the fact came from.
    pub origin: LicenseOrigin,
}

impl LicenseFact {
    /// The fact of a component whose document said nothing about the
    /// license. It is the one fact that must never be "improved": an
    /// absent license stays absent.
    pub fn unknown(component: &str) -> Self {
        Self {
            component: component.to_string(),
            expression: None,
            identifiers: Vec::new(),
            listed: Vec::new(),
            deprecated: Vec::new(),
            origin: LicenseOrigin::Absent,
        }
    }

    /// Whether the document said nothing about the license.
    pub fn is_unknown(&self) -> bool {
        self.origin == LicenseOrigin::Absent
    }
}

/// Reads an SPDX license expression into its tokens: `MIT OR
/// Apache-2.0` gives the identifiers `MIT` and `Apache-2.0`; `MPL-2.0
/// AND GPL-2.0-only WITH Classpath-exception-2.0` gives `MPL-2.0` and
/// `GPL-2.0-only` -- the exception belongs to the license before it and
/// is kept inside the expression, which travels whole.
///
/// An expression that cannot be read as tokens (`NOASSERTION`, `NONE`,
/// or anything malformed) gives no tokens: the reading of a license is
/// not the place where a default is invented.
pub fn parse_expression(expression: &str) -> Vec<String> {
    let expression = expression.trim();
    if expression.is_empty()
        || expression.eq_ignore_ascii_case("NOASSERTION")
        || expression.eq_ignore_ascii_case("NONE")
    {
        return Vec::new();
    }

    // The operator and grouping words of an SPDX expression carry no
    // license, and neither do the parentheses. What is left are the
    // identifiers; a `WITH` clause keeps its exception with the license
    // it modifies, so the exception word and the word after it are
    // skipped together.
    let mut identifiers = Vec::new();
    let mut skip_exception = false;
    for token in expression
        .split(['(', ')'])
        .flat_map(|part| part.split_whitespace())
    {
        if skip_exception {
            skip_exception = false;
            continue;
        }
        match token {
            "AND" | "OR" => continue,
            "WITH" => {
                skip_exception = true;
                continue;
            }
            _ => {
                // A token that is not one of the operators and not an
                // exception: a license identifier, or something that is
                // not one. A name that is not an SPDX-shaped identifier
                // (lowercase words, punctuation) stops the reading: the
                // expression carried a license Hodei cannot name, and
                // guessing it would be inventing.
                if !is_spdx_id_shape(token) {
                    return Vec::new();
                }
                identifiers.push(token.to_string());
            }
        }
    }
    identifiers
}

/// Whether the token has the shape an SPDX license identifier has.
///
/// The published identifiers start with an uppercase letter or a digit
/// -- `MIT`, `Apache-2.0`, `0BSD` -- carry letters, digits, dot, dash
/// and plus inside, and an ordinary lowercase word is not one of them.
/// The shape is a guard against reading the words of a sentence as
/// licenses; being in the snapshot is the thing that says an
/// identifier exists, and that is the boolean on the fact.
fn is_spdx_id_shape(token: &str) -> bool {
    !token.is_empty()
        && !token.eq_ignore_ascii_case("NOASSERTION")
        && !token.eq_ignore_ascii_case("NONE")
        && token
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
}

/// The snapshot of the SPDX license list this module reads against.
///
/// It is a curated subset of the published list -- the identifiers the
/// fixtures and the inventories of the workspace carry, plus a few of
/// the deprecation notes. A license outside the snapshot is still a
/// license the document names; the fact says `Listed: false` so the
/// reader sees that the snapshot did not know it, which is the honest
/// thing: not that the license does not exist.
#[derive(Debug, Clone, Default)]
pub struct SpdxList {
    /// The identifiers the list carries, with their deprecation note.
    entries: Vec<(&'static str, bool)>,
}

impl SpdxList {
    /// The snapshot Hodei ships.
    pub fn snapshot() -> Self {
        Self {
            entries: vec![
                ("MIT", false),
                ("Apache-2.0", false),
                ("BSD-2-Clause", false),
                ("BSD-3-Clause", false),
                ("GPL-2.0-only", false),
                ("GPL-3.0-only", false),
                ("LGPL-2.1-only", false),
                ("LGPL-3.0-only", false),
                ("MPL-2.0", false),
                ("ISC", false),
                ("0BSD", false),
                ("Unlicense", false),
                // The deprecated identifiers keep their note from the
                // list: they are named, and the list says they are old.
                ("GPL-2.0", true),
                ("GPL-3.0", true),
                ("LGPL-2.1", true),
                ("GPL-1.0+", true),
            ],
        }
    }

    /// Whether the snapshot knows the identifier.
    pub fn is_listed(&self, identifier: &str) -> bool {
        self.entries.iter().any(|(id, _)| *id == identifier)
    }

    /// Whether the snapshot carries the deprecation note for it.
    pub fn is_deprecated(&self, identifier: &str) -> bool {
        self.entries
            .iter()
            .find(|(id, _)| *id == identifier)
            .is_some_and(|(_, deprecated)| *deprecated)
    }

    /// The fact a document with this expression states about a
    /// component. The expression travels as it came and the tokens are
    /// checked against the snapshot.
    ///
    /// An expression without license tokens (`NOASSERTION`, `NONE`, a
    /// name that is not one) states NO license: the fact keeps what the
    /// document wrote and records the origin as absent, because
    /// inventing a license for it is what this module exists to refuse.
    pub fn fact_of(&self, component: &str, expression: &str) -> LicenseFact {
        let identifiers = parse_expression(expression);
        let origin = if identifiers.is_empty() {
            LicenseOrigin::Absent
        } else {
            LicenseOrigin::Expression
        };
        LicenseFact {
            component: component.to_string(),
            expression: (!expression.trim().is_empty()).then(|| expression.trim().to_string()),
            listed: identifiers
                .iter()
                .map(|identifier| self.is_listed(identifier))
                .collect(),
            deprecated: identifiers
                .iter()
                .map(|identifier| self.is_deprecated(identifier))
                .collect(),
            identifiers,
            origin,
        }
    }
}

/// A license the document named by a file, kept as what it is: a
/// reference the inventory states, not a license decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseFile {
    /// The component that carries it.
    pub component: String,
    /// Where the document says the license text is.
    pub path: String,
}

impl LicenseFile {
    /// The fact of a license stated by a file.
    pub fn fact(&self) -> LicenseFact {
        LicenseFact {
            component: self.component.clone(),
            expression: None,
            identifiers: Vec::new(),
            listed: Vec::new(),
            deprecated: Vec::new(),
            origin: LicenseOrigin::File,
        }
    }
}

/// The inventory's license facts: what every component states, and what
/// it does not.
///
/// `record` is the seam the projection uses: the component of the
/// inventory and its expression -- when the inventory has one -- give
/// the fact. An expression that is not there gives the unknown fact,
/// and nothing fills it in.
#[derive(Debug, Clone)]
pub struct LicenseLedger {
    facts: Vec<LicenseFact>,
}

impl LicenseLedger {
    /// A ledger that starts empty.
    pub fn new() -> Self {
        Self { facts: Vec::new() }
    }

    /// Records what the document of one component says.
    pub fn record(&mut self, list: &SpdxList, component: &str, expression: Option<&str>) {
        let fact = match expression {
            Some(expression) if !expression.trim().is_empty() => {
                list.fact_of(component, expression)
            }
            _ => LicenseFact::unknown(component),
        };
        self.facts.push(fact);
    }

    /// The fact of a component, when the ledger has one.
    pub fn of(&self, component: &str) -> Option<&LicenseFact> {
        self.facts.iter().find(|fact| fact.component == component)
    }

    /// The facts themselves.
    pub fn facts(&self) -> &[LicenseFact] {
        &self.facts
    }

    /// The components whose license is not known. They are counted and
    /// named: an unknown license is a gap the report says, and it is
    /// never counted as permissive or as copyleft by default.
    pub fn unknown_components(&self) -> Vec<&str> {
        self.facts
            .iter()
            .filter(|fact| fact.is_unknown())
            .map(|fact| fact.component.as_str())
            .collect()
    }
}

impl Default for LicenseLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expression_is_split_into_its_tokens_and_keeps_whole() {
        let list = SpdxList::snapshot();
        let fact = list.fact_of("org.example:lib", "MIT OR Apache-2.0");
        assert_eq!(fact.expression.as_deref(), Some("MIT OR Apache-2.0"));
        assert_eq!(fact.identifiers, vec!["MIT", "Apache-2.0"]);
        assert_eq!(fact.listed, vec![true, true]);
        assert_eq!(fact.deprecated, vec![false, false]);
    }

    #[test]
    fn an_exception_travels_with_the_license_it_modifies() {
        let list = SpdxList::snapshot();
        let fact = list.fact_of("lib", "GPL-2.0-only WITH Classpath-exception-2.0");
        assert_eq!(fact.identifiers, vec!["GPL-2.0-only"]);
        assert!(
            fact.expression
                .as_deref()
                .is_some_and(|e| e.contains("WITH Classpath-exception-2.0")),
            "the exception stays in the expression"
        );
    }

    #[test]
    fn a_grouped_expression_gives_all_its_identifiers() {
        let list = SpdxList::snapshot();
        let fact = list.fact_of("lib", "(MIT OR Apache-2.0) AND BSD-3-Clause");
        assert_eq!(fact.identifiers, vec!["MIT", "Apache-2.0", "BSD-3-Clause"]);
    }

    #[test]
    fn a_deprecated_identifier_says_so_and_a_listed_one_does_not() {
        let list = SpdxList::snapshot();
        let fact = list.fact_of("lib", "GPL-2.0");
        assert_eq!(fact.deprecated, vec![true], "the list says it is old");
        assert!(fact.listed[0], "and it is in the list");

        let modern = list.fact_of("lib", "MIT");
        assert_eq!(modern.deprecated, vec![false]);
    }

    #[test]
    fn a_license_outside_the_snapshot_is_named_and_marked_as_unlisted() {
        let list = SpdxList::snapshot();
        let fact = list.fact_of("lib", "ZLib");
        assert_eq!(fact.identifiers, vec!["ZLib"], "the document named it");
        assert_eq!(fact.listed, vec![false], "the snapshot does not carry it");
    }

    #[test]
    fn an_absent_license_stays_absent() {
        let mut ledger = LicenseLedger::new();
        let list = SpdxList::snapshot();
        ledger.record(&list, "org.example:lib-b", None);
        ledger.record(&list, "org.example:lib-c", Some("  "));

        for component in ["org.example:lib-b", "org.example:lib-c"] {
            let fact = ledger.of(component).expect("the component is there");
            assert!(fact.is_unknown(), "the license stays unknown");
            assert!(
                fact.identifiers.is_empty(),
                "and nothing is invented for it"
            );
        }
        assert_eq!(ledger.unknown_components().len(), 2);
    }

    #[test]
    fn a_noassertion_expression_is_read_as_no_tokens() {
        let list = SpdxList::snapshot();
        for text in ["NOASSERTION", "NONE", "not a license"] {
            let fact = list.fact_of("lib", text);
            assert!(
                fact.identifiers.is_empty(),
                "`{text}` is not a license and is not given one"
            );
        }
    }

    #[test]
    fn a_file_license_is_recorded_as_a_file() {
        let file = LicenseFile {
            component: "org.example:lib-d".to_string(),
            path: "LICENSES/MIT.txt".to_string(),
        };
        let fact = file.fact();
        assert_eq!(fact.origin, LicenseOrigin::File);
        assert!(
            !fact.is_unknown(),
            "a file is said by the document: it is not an absent license"
        );
    }
}
