//! The security case: the object that tracks a finding until somebody
//! proves it is fixed.
//!
//! A finding on its own is a fact and not a piece of work. The security
//! case is the piece of work: it says what triggered it, what is affected,
//! how far the damage reaches, who owns it, and **what would prove it
//! fixed**.
//!
//! The case has a **timeline**, and the timeline is the reason the case
//! exists as an object and not as a row that gets overwritten: the state
//! of a case is derived from the newest evidence, but the trigger that
//! opened it is still there, and so is every change in between.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::affected::{AffectedState, Finding};

/// The state of a security case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseState {
    /// It was opened and nobody has worked on it.
    Open,
    /// Somebody is working on it.
    Mitigating,
    /// The remediation is claimed.
    Remediated,
    /// The remediation was verified against the deployments.
    VerifiedFixed,
    /// The finding does not apply.
    ClosedNotAffected,
}

impl CaseState {
    /// The name of the state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Mitigating => "mitigating",
            Self::Remediated => "remediated",
            Self::VerifiedFixed => "verified_fixed",
            Self::ClosedNotAffected => "closed_not_affected",
        }
    }

    /// Whether the case is closed.
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::VerifiedFixed | Self::ClosedNotAffected)
    }
}

/// What happened in the case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseEvent {
    /// What happened.
    pub kind: CaseEventKind,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// What it says.
    pub detail: String,
}

/// The kinds of event of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseEventKind {
    /// The case was opened.
    Opened,
    /// A piece of evidence arrived.
    Evidence,
    /// The state changed.
    StateChanged,
    /// Somebody took ownership.
    Assigned,
    /// The remediation was verified.
    Verified,
}

/// The criteria that would prove the case fixed.
///
/// They are written down when the case is opened, because "fixed" is a
/// word and a criterion is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCriteria {
    /// The artefact that replaces the affected one.
    pub replacement_artifact: Option<String>,
    /// The deployments that have to stop running the affected one.
    pub deployments_removed: Vec<String>,
    /// Whether the deployment evidence is required.
    ///
    /// It is required: a closed issue without the evidence that the
    /// affected artefact stopped running is a hope, and a hope does not
    /// close a case.
    pub requires_deployment_evidence: bool,
}

impl VerificationCriteria {
    /// The criteria of a case.
    pub fn new(replacement_artifact: Option<String>) -> Self {
        Self {
            replacement_artifact,
            deployments_removed: Vec::new(),
            requires_deployment_evidence: true,
        }
    }

    /// The deployments that have to stop running the affected artefact.
    pub fn with_deployments_removed(mut self, deployments: Vec<String>) -> Self {
        self.deployments_removed = deployments;
        self
    }
}

/// The snapshot of how far the damage reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlastRadius {
    /// The artefacts that are affected.
    pub affected_subjects: Vec<String>,
    /// The deployments that run them.
    pub affected_deployments: Vec<String>,
    /// The components the vulnerability is in.
    pub affected_components: Vec<String>,
}

/// The security case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityCase {
    /// The identifier of the case.
    pub id: String,
    /// The vulnerability that opened it.
    pub trigger: String,
    /// The state of the case.
    pub state: CaseState,
    /// Who owns it.
    pub owner: Option<String>,
    /// The findings the case covers.
    pub findings: Vec<Finding>,
    /// The snapshot of how far the damage reaches.
    pub blast_radius: BlastRadius,
    /// What would prove it fixed.
    pub criteria: VerificationCriteria,
    /// The timeline.
    pub timeline: Vec<CaseEvent>,
    /// When the case was opened.
    pub opened_at: DateTime<Utc>,
}

impl SecurityCase {
    /// Opens a case.
    ///
    /// The trigger and the findings go in, and the timeline starts with
    /// the opening. The case is created once: asking for a case that
    /// already exists returns the one that is there, because a case that
    /// is opened twice for the same thing is two cases and nobody owns
    /// either.
    pub fn open(
        id: impl Into<String>,
        trigger: impl Into<String>,
        findings: Vec<Finding>,
        blast_radius: BlastRadius,
        criteria: VerificationCriteria,
        now: DateTime<Utc>,
    ) -> Self {
        let trigger = trigger.into();
        let mut case = Self {
            id: id.into(),
            timeline: Vec::new(),
            trigger: trigger.clone(),
            state: CaseState::Open,
            owner: None,
            findings,
            blast_radius,
            criteria,
            opened_at: now,
        };
        case.timeline.push(CaseEvent {
            kind: CaseEventKind::Opened,
            at: now,
            detail: format!("the case was opened by {trigger}"),
        });
        case
    }

    /// The identifier of the case that a trigger opens.
    ///
    /// It is derived from the trigger, so that opening the case twice for
    /// the same vulnerability gives the same identifier, and the store
    /// keeps one.
    pub fn id_for(trigger: &str) -> String {
        format!("case-{trigger}")
    }

    /// Records that somebody took ownership.
    pub fn assign(&mut self, owner: impl Into<String>, now: DateTime<Utc>) {
        let owner = owner.into();
        self.timeline.push(CaseEvent {
            kind: CaseEventKind::Assigned,
            at: now,
            detail: format!("the case was assigned to {owner}"),
        });
        self.owner = Some(owner);
        if self.state == CaseState::Open {
            self.state = CaseState::Mitigating;
        }
    }

    /// Records a piece of evidence that arrived.
    ///
    /// The state is derived from the evidence and the timeline keeps the
    /// change: the case that is there now and the case that was there
    /// before are both true.
    pub fn record_evidence(
        &mut self,
        finding: Finding,
        detail: impl Into<String>,
        now: DateTime<Utc>,
    ) {
        let detail = detail.into();
        self.timeline.push(CaseEvent {
            kind: CaseEventKind::Evidence,
            at: now,
            detail: detail.clone(),
        });

        // The finding that arrives REPLACES the one it talks about: the
        // case knows one finding per component and advisory, and the new
        // evidence says what that finding is now. The finding that was
        // there is not erased from the case -- it is in the timeline.
        match self.findings.iter_mut().find(|existing| {
            existing.advisory_id == finding.advisory_id && existing.component == finding.component
        }) {
            Some(existing) => *existing = finding,
            None => self.findings.push(finding),
        }

        // The state follows the evidence: a finding that is no longer
        // affected closes the case as not affected, and a finding that is
        // affected keeps it open.
        let all_unaffected = self
            .findings
            .iter()
            .all(|existing| existing.state == AffectedState::Unaffected);
        let any_affected = self
            .findings
            .iter()
            .any(|existing| existing.state == AffectedState::Affected);

        let new_state = if all_unaffected && !any_affected {
            CaseState::ClosedNotAffected
        } else if any_affected && self.state != CaseState::VerifiedFixed {
            if self.state == CaseState::Open {
                CaseState::Open
            } else {
                self.state
            }
        } else {
            self.state
        };

        if new_state != self.state {
            self.timeline.push(CaseEvent {
                kind: CaseEventKind::StateChanged,
                at: now,
                detail: format!(
                    "the state changed from {} to {}",
                    self.state.as_str(),
                    new_state.as_str()
                ),
            });
            self.state = new_state;
        }
    }

    /// Records that the remediation was verified.
    ///
    /// The verification needs the deployment evidence: a claim that the
    /// affected artefact stopped running is not evidence that it stopped
    /// running.
    pub fn verify_remediation(
        &mut self,
        replacement_artifact: &str,
        deployments_without_the_affected: &[String],
        now: DateTime<Utc>,
    ) -> CaseState {
        let criteria_met = self.criteria.replacement_artifact.as_deref()
            == Some(replacement_artifact)
            && deployments_without_the_affected.len() >= self.criteria.deployments_removed.len();

        if !criteria_met {
            self.timeline.push(CaseEvent {
                kind: CaseEventKind::Evidence,
                at: now,
                detail: "the verification did not meet the criteria of the case".to_string(),
            });
            return self.state;
        }

        self.timeline.push(CaseEvent {
            kind: CaseEventKind::Verified,
            at: now,
            detail: format!(
                "the remediation was verified with {replacement_artifact} and {} deployments without the affected artefact",
                deployments_without_the_affected.len()
            ),
        });
        self.timeline.push(CaseEvent {
            kind: CaseEventKind::StateChanged,
            at: now,
            detail: format!(
                "the state changed from {} to {}",
                self.state.as_str(),
                CaseState::VerifiedFixed.as_str()
            ),
        });
        self.state = CaseState::VerifiedFixed;
        self.state
    }

    /// The state of the case, derived from the newest evidence.
    pub fn current_state(&self) -> CaseState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affected::{DependencyKind, Finding};

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

    fn a_case() -> SecurityCase {
        let now = Utc::now();
        SecurityCase::open(
            "case-1",
            "GHSA-test-0001",
            vec![a_finding(AffectedState::Affected)],
            BlastRadius {
                affected_subjects: vec![
                    "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web".to_string(),
                ],
                affected_deployments: vec!["prod".to_string()],
                affected_components: vec!["org.example:lib-b".to_string()],
            },
            VerificationCriteria::new(Some("sha256:new".to_string()))
                .with_deployments_removed(vec!["prod".to_string()]),
            now,
        )
    }

    #[test]
    fn a_case_opens_with_its_trigger_and_its_findings() {
        let case = a_case();
        assert_eq!(case.state, CaseState::Open);
        assert_eq!(case.trigger, "GHSA-test-0001");
        assert_eq!(case.findings.len(), 1);
        assert_eq!(case.timeline.len(), 1);
        assert_eq!(case.timeline[0].kind, CaseEventKind::Opened);
    }

    #[test]
    fn the_evidence_updates_the_case_without_erasing_the_history() {
        let now = Utc::now();
        let mut case = a_case();
        let opened_at = case.opened_at;

        case.record_evidence(
            a_finding(AffectedState::Unaffected),
            "a VEX statement says it is not affected",
            now,
        );

        assert_eq!(
            case.timeline.len(),
            3,
            "the opening, the evidence and the change of state are all there"
        );
        assert_eq!(case.timeline[0].kind, CaseEventKind::Opened);
        assert_eq!(case.opened_at, opened_at, "and the trigger is still there");
        assert_eq!(case.trigger, "GHSA-test-0001");
        assert_eq!(
            case.findings.len(),
            1,
            "and the case knows one finding per component, which is now up to date"
        );
        assert_eq!(
            case.findings[0].state,
            AffectedState::Unaffected,
            "and the state derives from the new evidence"
        );
    }

    #[test]
    fn the_verified_remediation_closes_the_case() {
        let now = Utc::now();
        let mut case = a_case();
        let state = case.verify_remediation("sha256:new", &["prod".to_string()], now);
        assert_eq!(state, CaseState::VerifiedFixed);
        assert!(case.state.is_closed());
    }

    #[test]
    fn a_closed_issue_without_the_deployment_evidence_does_not_close_the_case() {
        let now = Utc::now();
        let mut case = a_case();
        // The artefact is there but the deployment evidence is not.
        let state = case.verify_remediation("sha256:new", &[], now);
        assert_ne!(
            state,
            CaseState::VerifiedFixed,
            "a claim that the affected artefact stopped running is not evidence that it did"
        );
        assert_eq!(case.state, CaseState::Open);
    }
}
