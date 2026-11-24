//! The explicit policy that turns an observation into a decision.
//!
//! The detectors classify; the policy acts, and the two are separate
//! objects on purpose. The rules of the action side, in order:
//!
//! - **The default is `Detect`:** alert, never hold. A deployment that
//!   installs nothing because a name looked similar is a deployment
//!   that learned the wrong lesson — the block would train people to
//!   turn the detector off.
//! - **`Hold` quarantines only through an authorized rule.** The rule
//!   is the declared namespace reservation (HSP-143): the organization
//!   already decided, explicitly, that those names are its own and are
//!   never served from outside. A publication inside a reservation from
//!   a publisher the organization has not seen is the one similarity-
//!   free case where holding is authorized. **Name similarity never
//!   authorizes a hold**, in any mode and at any score: a typosquat
//!   signal raises an alert, and a human decides.
//! - **A quarantine opens a case, and the case finding is `Unknown`.**
//!   The heuristic has not proved anything; the case records what would
//!   prove the suspicion wrong or right, and the state says "nobody
//!   knows" until a review closes it.

use serde::{Deserialize, Serialize};

use super::catalog::Ecosystem;
use super::signals::{Observation, ReasonCode};

/// How the policy treats what it sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    /// Alert and keep serving. The default, and the only mode that is
    /// safe when the classification is a hypothesis.
    #[default]
    Detect,
    /// Hold (quarantine) the releases that an AUTHORIZED rule covers —
    /// and nothing else. Everything else alerts as in `Detect`.
    Hold,
}

/// What the platform does with a publication once the observation is
/// made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Nothing to do; the release is normal.
    Allow,
    /// The release is served but the signals go to a reviewer.
    Alert,
    /// The release is held from serving pending a review. Only an
    /// authorized rule can produce it.
    Quarantine,
}

impl Decision {
    /// The name of the decision.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Alert => "alert",
            Self::Quarantine => "quarantine",
        }
    }

    /// Whether the decision keeps the package away from consumers.
    pub fn blocks(&self) -> bool {
        matches!(self, Self::Quarantine)
    }
}

/// The explicit heuristic policy of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeuristicPolicy {
    /// Detect or hold.
    pub mode: PolicyMode,
    /// The aggregate score at or above which an observation is worth an
    /// alert. It is published with the dataset metrics.
    pub alert_threshold: u16,
}

impl Default for HeuristicPolicy {
    /// The default is fail-open in action: detect, do not hold.
    fn default() -> Self {
        Self {
            mode: PolicyMode::Detect,
            alert_threshold: super::signals::HYPOTHESIS_THRESHOLD,
        }
    }
}

impl HeuristicPolicy {
    /// A new policy from a mode and a threshold.
    pub fn new(mode: PolicyMode, alert_threshold: u16) -> Self {
        Self {
            mode,
            alert_threshold,
        }
    }

    /// The hold mode, configured explicitly. Choosing it is not
    /// enough to hold anything: only an authorized rule holds.
    pub fn hold(alert_threshold: u16) -> Self {
        Self {
            mode: PolicyMode::Hold,
            alert_threshold,
        }
    }
}

/// The decision with its justification: what was chosen, why, and
/// which rule — if any — authorized an action with teeth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// The decision.
    pub decision: Decision,
    /// The aggregate score the observation carried.
    pub score: u16,
    /// The reason codes that authorized the decision.
    ///
    /// `Quarantine` has a non-empty authorization set; `Alert` may
    /// alert on scores and similarities ALONE — nothing authorized by
    /// similarity here, similarity never authorizes.
    pub authorized_by: Vec<ReasonCode>,
    /// The explanation, naming the mode and the rule.
    pub explanation: String,
}

/// Decides what the policy does with an observation.
///
/// Pure, and fail-open: with no explicit configuration the answer to
/// any similarity is an alert, never a hold. The only path to
/// `Quarantine` is the authorized rule — the declared reservation of
/// HSP-143 firing from an unknown publisher — and it has to be in
/// `Hold` mode for the mode to have been chosen at all.
#[must_use]
pub fn decide(policy: &HeuristicPolicy, observation: &Observation) -> Verdict {
    let reserved = observation
        .reasons
        .contains(&ReasonCode::ReservedNamespaceFromUnknownPublisher);
    let alerting = observation.score >= policy.alert_threshold;

    if policy.mode == PolicyMode::Hold && reserved {
        return Verdict {
            decision: Decision::Quarantine,
            score: observation.score,
            authorized_by: vec![ReasonCode::ReservedNamespaceFromUnknownPublisher],
            explanation: format!(
                "hold mode: `{}` lands in a declared namespace reservation from an \
                 unknown publisher — the reservation, not a similarity, authorized the quarantine",
                observation.record.name
            ),
        };
    }

    if alerting {
        return Verdict {
            decision: Decision::Alert,
            score: observation.score,
            authorized_by: Vec::new(),
            explanation: format!(
                "{} mode: score {} is at or over the alert threshold {}; the release keeps \
                 serving and the signals go to a reviewer",
                mode_name(policy.mode),
                observation.score,
                policy.alert_threshold
            ),
        };
    }

    Verdict {
        decision: Decision::Allow,
        score: observation.score,
        authorized_by: Vec::new(),
        explanation: format!(
            "score {} is under the alert threshold {}; nothing to do",
            observation.score, policy.alert_threshold
        ),
    }
}

fn mode_name(mode: PolicyMode) -> &'static str {
    match mode {
        PolicyMode::Detect => "detect",
        PolicyMode::Hold => "hold",
    }
}

/// Opens the security case a quarantine deserves.
///
/// The trigger names the ecosystem, the package and the rule that held
/// it; the finding is `Unknown` — the heuristic holds the release from
/// serving while it is reviewed, and it has NOT proved the package
/// malicious. The criteria stay empty on purpose: a heuristic cannot
/// write what would prove the fix, so only a review — the feedback
/// path — can close the case.
pub fn quarantine_case(
    observation: &Observation,
    _verdict: &Verdict,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::case::SecurityCase {
    let trigger = format!(
        "advisory-{}:{}@{}",
        ecosystem_tag(observation.record.ecosystem),
        observation.record.name,
        observation.record.version
    );
    let finding = crate::affected::Finding {
        advisory_id: trigger.clone(),
        component: observation.record.name.clone(),
        version: Some(observation.record.version.clone()),
        // The whole point of the epistemic rule: a heuristic holds, it
        // does not accuse. The state is "nobody knows" — and "nobody
        // knows" is not "affected".
        state: crate::affected::AffectedState::Unknown,
        dependency: crate::affected::DependencyKind::Direct,
        vex: None,
    };
    crate::case::SecurityCase::open(
        crate::case::SecurityCase::id_for(&trigger),
        &trigger,
        vec![finding],
        crate::case::BlastRadius {
            affected_subjects: Vec::new(),
            affected_deployments: Vec::new(),
            affected_components: vec![observation.record.name.clone()],
            depths: Vec::new(),
        },
        crate::case::VerificationCriteria::new(None),
        now,
    )
}

fn ecosystem_tag(ecosystem: Ecosystem) -> &'static str {
    match ecosystem {
        Ecosystem::Npm => "npm",
        Ecosystem::Pypi => "pypi",
        Ecosystem::Maven => "maven",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisories::catalog::{EcosystemCatalog, KnownMetadata, KnownPackage};
    use crate::advisories::signals::{PackageRecord, evaluate};
    use chrono::{TimeZone, Utc};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 0, 0, 0).unwrap()
    }

    fn acme_catalog() -> EcosystemCatalog {
        EcosystemCatalog::new(
            vec![KnownPackage {
                name: "@acme/utils".to_string(),
                publisher: "acme-ci".to_string(),
                versions: vec!["1.0.0".to_string()],
                last_metadata: KnownMetadata {
                    publisher_age_days: 365,
                    ..Default::default()
                },
            }],
            vec!["@acme".to_string()],
        )
    }

    fn confusion_observation() -> Observation {
        let record = PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "@acme/utils".to_string(),
            version: "2.0.0".to_string(),
            publisher: "intruder".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 1,
                ..Default::default()
            },
        };
        evaluate(&record, &acme_catalog())
    }

    fn typosquat_observation() -> Observation {
        let record = PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "raect".to_string(),
            version: "0.1.0".to_string(),
            publisher: "someone".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 365,
                ..Default::default()
            },
        };
        let catalog = EcosystemCatalog::new(
            vec![KnownPackage {
                name: "react".to_string(),
                publisher: "facebook".to_string(),
                versions: vec!["18.0.0".to_string()],
                last_metadata: KnownMetadata {
                    publisher_age_days: 365,
                    ..Default::default()
                },
            }],
            Vec::new(),
        );
        evaluate(&record, &catalog)
    }

    #[test]
    fn the_default_policy_alerts_and_never_holds() {
        let policy = HeuristicPolicy::default();
        assert_eq!(policy.mode, PolicyMode::Detect);
        // The loudest observation the detectors can make without an
        // authorized rule: even a reserved namespace in the default
        // mode only alerts.
        let verdict = decide(&policy, &confusion_observation());
        assert_eq!(verdict.decision, Decision::Alert);
        assert!(
            !verdict.decision.blocks(),
            "fail-open in action is the default"
        );
    }

    #[test]
    fn hold_mode_quarantines_only_authorized_reservations() {
        let policy = HeuristicPolicy::hold(50);
        let observation = confusion_observation();
        let verdict = decide(&policy, &observation);
        assert_eq!(verdict.decision, Decision::Quarantine);
        assert_eq!(
            verdict.authorized_by,
            vec![ReasonCode::ReservedNamespaceFromUnknownPublisher]
        );
        assert!(verdict.explanation.contains("reservation"));
    }

    #[test]
    fn name_similarity_never_quarantines_in_any_mode() {
        let policy = HeuristicPolicy::hold(1); // even a threshold of one
        let observation = typosquat_observation();
        let verdict = decide(&policy, &observation);
        assert_eq!(
            verdict.decision,
            Decision::Alert,
            "a score over the threshold with only similarity signals is an alert"
        );
        assert!(
            !verdict
                .authorized_by
                .contains(&ReasonCode::NameSimilarToKnownPackage)
        );
        assert!(
            !verdict.decision.blocks(),
            "and an unauthorized similarity never blocks"
        );
    }

    #[test]
    fn a_quiet_release_is_allowed() {
        let policy = HeuristicPolicy::default();
        let catalog = EcosystemCatalog::new(Vec::new(), Vec::new());
        let record = PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "totally-normal".to_string(),
            version: "1.0.0".to_string(),
            publisher: "normal-dev".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 400,
                ..Default::default()
            },
        };
        let observation = evaluate(&record, &catalog);
        let verdict = decide(&policy, &observation);
        assert_eq!(verdict.decision, Decision::Allow);
    }

    #[test]
    fn a_quarantine_opens_a_case_that_says_nobody_knows() {
        let policy = HeuristicPolicy::hold(50);
        let observation = confusion_observation();
        let verdict = decide(&policy, &observation);
        let case = quarantine_case(&observation, &verdict, now());
        assert!(case.trigger.starts_with("advisory-npm:@acme/utils@2.0.0"));
        assert_eq!(
            case.findings[0].state,
            crate::affected::AffectedState::Unknown
        );
        assert_eq!(case.state, crate::case::CaseState::Open);
        assert_eq!(case.timeline[0].kind, crate::case::CaseEventKind::Opened);
    }
}
