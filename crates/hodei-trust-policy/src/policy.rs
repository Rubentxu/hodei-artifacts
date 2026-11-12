//! The policy: what a promotion has to meet.
//!
//! A trust policy is a **set of requirements**, and the set has a version.
//! The requirements are data and not code, so a deployment can change them
//! without changing the binary, and a decision can say which set it was
//! measured against.
//!
//! Each requirement says three things: **what** it asks for, **what happens
//! when it is not met** -- a denial or a request for approval, which the
//! policy decides and not the engine -- and **which exception** may waive
//! it. The reading of each kind of requirement is where the trust
//! knowledge lives.

use serde::{Deserialize, Serialize};

use crate::context::Exposure;

/// What happens when a requirement is not met.
///
/// The policy decides this and not the engine, because whether a missing
/// SBOM is a denial or a request for approval is a matter of the
/// organisation and not of the software. The two are told apart because
/// they lead to different places, and a decision that answered "deny" for
/// something that wanted an approval would send the promotion down the
/// wrong road.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    /// The promotion is refused.
    Deny,
    /// The promotion waits for somebody's approval.
    RequireApproval,
}

/// What a requirement asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequirementKind {
    /// The dimension has to be in one of the accepted states.
    ///
    /// The states that are accepted are named, so that a policy that says
    /// "the SBOM has to be complete" says *complete* and not "not bad".
    Dimension {
        /// The dimension.
        dimension: hodei_evidence::domain::capsule::DimensionName,
        /// The states that satisfy the requirement.
        accepts: Vec<hodei_evidence::domain::capsule::DimensionState>,
    },
    /// No vulnerability of critical severity may be reachable.
    NoReachableCriticalVulnerability,
    /// The risk has to have been established.
    ///
    /// A policy that allows promoting without knowing the risk is a policy
    /// that promotes on ignorance, and the requirement exists to say that
    /// out loud.
    RiskEstablished,
    /// The exposure of the deployment may not be the given one.
    ExposureAtMost {
        /// The exposure that is not allowed.
        exposure: Exposure,
    },
}

/// One requirement of a policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// What it asks for.
    pub kind: RequirementKind,
    /// What happens when it is not met.
    pub on_failure: OnFailure,
    /// The name of the exception that may waive it.
    ///
    /// An exception that is not named here does not waive this
    /// requirement, and an exception that has expired does not waive it
    /// either: a waiver that ran out is not a waiver.
    pub overridable_by: Option<String>,
}

impl Requirement {
    /// A requirement that refuses when it is not met.
    pub fn denying(kind: RequirementKind) -> Self {
        Self {
            kind,
            on_failure: OnFailure::Deny,
            overridable_by: None,
        }
    }

    /// A requirement that asks for an approval when it is not met.
    pub fn needing_approval(kind: RequirementKind) -> Self {
        Self {
            kind,
            on_failure: OnFailure::RequireApproval,
            overridable_by: None,
        }
    }

    /// The requirement may be waived by the named exception.
    pub fn overridable_by(mut self, exception: impl Into<String>) -> Self {
        self.overridable_by = Some(exception.into());
        self
    }

    /// The name of the requirement, which is what an exception waives.
    pub fn name(&self) -> String {
        match &self.kind {
            RequirementKind::Dimension { dimension, .. } => format!("dimension:{dimension}"),
            RequirementKind::NoReachableCriticalVulnerability => {
                "risk:reachable_critical".to_string()
            }
            RequirementKind::RiskEstablished => "risk:established".to_string(),
            RequirementKind::ExposureAtMost { .. } => "risk:exposure".to_string(),
        }
    }
}

/// A set of requirements, with the version that made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustPolicy {
    /// The identifier of the set.
    pub id: String,
    /// The version of the set.
    pub version: String,
    /// The version of the schema the set was written against.
    pub schema_version: String,
    /// The requirements, in the order they are written.
    pub requirements: Vec<Requirement>,
}

impl TrustPolicy {
    /// A policy set.
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        requirements: Vec<Requirement>,
    ) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            schema_version: crate::POLICY_SCHEMA_VERSION.to_string(),
            requirements,
        }
    }

    /// The requirement of a name, if the set has one.
    pub fn requirement(&self, name: &str) -> Option<&Requirement> {
        self.requirements
            .iter()
            .find(|requirement| requirement.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};

    #[test]
    fn a_requirement_has_a_name_that_an_exception_can_name() {
        let requirement = Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        });
        assert_eq!(requirement.name(), "dimension:inventory_completeness");
    }

    #[test]
    fn the_policy_decides_what_happens_when_a_requirement_is_not_met() {
        let requirement = Requirement::needing_approval(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        });
        assert_eq!(requirement.on_failure, OnFailure::RequireApproval);
    }
}
