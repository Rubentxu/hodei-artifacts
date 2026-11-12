//! The state of a finding: is this artefact affected, and by how much.
//!
//! The state has **three** values and not two, and the third one carries
//! the weight:
//!
//! - `Affected`: the artefact contains something the advisory is about.
//! - `Unaffected`: the evidence says it does not.
//! - `Unknown`: nobody knows, and "nobody knows" is not "unaffected".
//!
//! An artefact with no inventory has an **unknown** state. That is the
//! single most important rule of this module, and it is the one that
//! systems get wrong: without an inventory there is nothing to compare the
//! advisory against, and saying "not affected" would be saying that the
//! absence of evidence is evidence of absence.

use serde::{Deserialize, Serialize};

use crate::version::Match;
use crate::vex::VexStatement;

/// Whether an artefact is affected by an advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffectedState {
    /// The artefact contains something the advisory is about.
    Affected,
    /// The evidence says it does not.
    Unaffected,
    /// Nobody knows.
    Unknown,
}

impl AffectedState {
    /// The name of the state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Affected => "affected",
            Self::Unaffected => "unaffected",
            Self::Unknown => "unknown",
        }
    }

    /// Whether the state is an absence of knowledge.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// How the dependency is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// The artefact depends on the vulnerable component itself.
    Direct,
    /// The artefact depends on it through another dependency.
    Transitive,
}

/// The state of a finding about one component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The identifier of the advisory.
    pub advisory_id: String,
    /// The name of the component the advisory is about.
    pub component: String,
    /// The version of the component.
    pub version: Option<String>,
    /// Whether the artefact is affected.
    pub state: AffectedState,
    /// How the dependency is reached.
    pub dependency: DependencyKind,
    /// The VEX statement that moved the state, if there was one.
    pub vex: Option<VexReference>,
}

/// A reference to the VEX statement that was applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VexReference {
    /// The identifier of the statement.
    pub id: String,
    /// What the statement declared.
    pub status: crate::vex::VexStatus,
    /// Whether the statement was verified.
    ///
    /// A statement that nobody verified is a statement that somebody
    /// wrote: it can move the state, but it does not silence the finding
    /// on its own, and the finding keeps saying that it was there.
    pub verified: bool,
}

/// Decides the state of a finding, before the VEX is applied.
///
/// The inventory decides: without it there is nothing to compare, and the
/// answer is `Unknown`.
pub fn state_without_vex(
    advisory_id: &str,
    component: &str,
    version: Option<&str>,
    inventory_present: bool,
    match_result: Match,
    dependency: DependencyKind,
) -> Finding {
    let state = if !inventory_present {
        // No inventory: nobody knows. NOT "unaffected".
        AffectedState::Unknown
    } else {
        match match_result {
            Match::Affected => AffectedState::Affected,
            Match::Unaffected => AffectedState::Unaffected,
            Match::Unknown => AffectedState::Unknown,
        }
    };

    Finding {
        advisory_id: advisory_id.to_string(),
        component: component.to_string(),
        version: version.map(str::to_string),
        state,
        dependency,
        vex: None,
    }
}

/// Applies a VEX statement to a finding.
///
/// The rules are the ones that matter:
///
/// - A statement that says **not affected** moves the finding to
///   `Unaffected`, and it keeps the reference to the statement so that a
///   reader can see why.
/// - A statement that says **affected** moves it to `Affected`.
/// - A statement that says **fixed** moves it to `Unaffected` only if it
///   was verified: an unverified claim that something is fixed is a claim,
///   and a claim does not close a finding.
/// - A statement that says **under investigation** does not move the
///   finding: nobody knows yet, and the finding stays as it was.
/// - A statement that does not apply to this component does not move
///   anything.
pub fn apply_vex(finding: &mut Finding, statement: &VexStatement) {
    if statement.does_not_apply_to(&finding.component) {
        return;
    }

    let verified = statement.is_verified();
    let status = statement.status;

    let new_state = match status {
        crate::vex::VexStatus::NotAffected => Some(AffectedState::Unaffected),
        crate::vex::VexStatus::Affected => Some(AffectedState::Affected),
        crate::vex::VexStatus::Fixed => {
            if verified {
                Some(AffectedState::Unaffected)
            } else {
                // An unverified claim that something is fixed does not
                // close the finding.
                None
            }
        }
        crate::vex::VexStatus::UnderInvestigation => None,
    };

    let Some(new_state) = new_state else {
        return;
    };

    finding.state = new_state;
    finding.vex = Some(VexReference {
        id: statement.id.clone(),
        status,
        verified,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vex::{VexStatement, VexStatus};

    fn a_finding(state: AffectedState) -> Finding {
        Finding {
            advisory_id: "GHSA-test-0001".to_string(),
            component: "org.example:lib-b".to_string(),
            version: Some("2.0.0".to_string()),
            state,
            dependency: DependencyKind::Transitive,
            vex: None,
        }
    }

    #[test]
    fn an_artefact_without_an_inventory_is_unknown_and_not_safe() {
        let finding = state_without_vex(
            "GHSA-test-0001",
            "org.example:lib-b",
            Some("2.0.0"),
            false,
            Match::Unaffected,
            DependencyKind::Direct,
        );
        assert_eq!(
            finding.state,
            AffectedState::Unknown,
            "without an inventory nobody knows, and that is not 'unaffected'"
        );
    }

    #[test]
    fn a_verified_statement_that_says_not_affected_moves_the_finding() {
        let mut finding = a_finding(AffectedState::Affected);
        let statement =
            VexStatement::not_affected("vex-1", "org.example:lib-b", VexStatus::NotAffected, true);
        apply_vex(&mut finding, &statement);
        assert_eq!(finding.state, AffectedState::Unaffected);
        assert_eq!(finding.vex.expect("the reference is there").id, "vex-1");
    }

    #[test]
    fn an_unverified_statement_that_says_fixed_does_not_close_the_finding() {
        let mut finding = a_finding(AffectedState::Affected);
        let statement =
            VexStatement::not_affected("vex-1", "org.example:lib-b", VexStatus::Fixed, false);
        apply_vex(&mut finding, &statement);
        assert_eq!(
            finding.state,
            AffectedState::Affected,
            "an unverified claim that something is fixed does not close a finding"
        );
        assert!(
            finding.vex.is_none(),
            "and it is not even recorded as having moved it"
        );
    }

    #[test]
    fn a_statement_about_another_component_moves_nothing() {
        let mut finding = a_finding(AffectedState::Affected);
        let statement =
            VexStatement::not_affected("vex-1", "org.example:other", VexStatus::NotAffected, true);
        apply_vex(&mut finding, &statement);
        assert_eq!(finding.state, AffectedState::Affected);
    }

    #[test]
    fn a_statement_that_is_still_under_investigation_moves_nothing() {
        let mut finding = a_finding(AffectedState::Affected);
        let statement = VexStatement::not_affected(
            "vex-1",
            "org.example:lib-b",
            VexStatus::UnderInvestigation,
            true,
        );
        apply_vex(&mut finding, &statement);
        assert_eq!(finding.state, AffectedState::Affected);
    }
}
