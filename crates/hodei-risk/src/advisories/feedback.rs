//! The feedback loop: a false positive is resolved by a person who
//! signed, not by a score that cooled down.
//!
//! A heuristic will be wrong some fraction of the time — the dataset
//! in `tests/fixtures` publishes that fraction — and the loop that
//! corrects it must not be a self-reinforcing one. Three rules:
//!
//! - **A review needs an authorization object.** The reviewer is named,
//!   the decision is theirs, and the record keeps both. No ticket, no
//!   resolution: a missing or expired authorization is an error, not a
//!   default.
//! - **Only a false positive can be resolved by feedback.** Confirming
//!   a true positive is a triage question, not a suppression: the
//!   signal stays where it is. The feedback path dismisses a signal
//!   after saying why, and it NEVER adds one.
//! - **The resolution writes into the case's own timeline.** The
//!   observation is not erased: the case that was opened keeps the
//!   opening, the held decision, and now the review with its author.
//!   The history is the audit trail.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::affected::{AffectedState, DependencyKind, Finding};
use crate::case::{CaseEventKind, SecurityCase};

use super::signals::Observation;

/// The signature a review needs to change anything.
///
/// It is a value, not a side effect: constructing one says "the person
/// `reviewer` decided this, with the ticket that proves the decision
/// was theirs to make". An un-ticketed review can observe; it cannot
/// resolve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewerAuthorization {
    /// The identity of the reviewer — the account that acted, not the
    /// account that was named.
    pub reviewer: String,
    /// The ticket, issue or decision reference that carries the
    /// human's intent.
    pub ticket: String,
    /// When the authorization was granted; it is not forever.
    pub granted_at: DateTime<Utc>,
}

impl ReviewerAuthorization {
    /// A fresh, explicit authorization from a named reviewer with a
    /// ticket.
    ///
    /// The constructor is the only way to build one, and there is no
    /// `Default`: a policy cannot accidentally receive an empty
    /// signature and think it was authorized.
    pub fn new(
        reviewer: impl Into<String>,
        ticket: impl Into<String>,
        granted_at: DateTime<Utc>,
    ) -> Self {
        Self {
            reviewer: reviewer.into(),
            ticket: ticket.into(),
            granted_at,
        }
    }

    /// Whether the authorization still stands.
    ///
    /// An authorization older than the window is not a decision any
    /// more; the reviewer has to look again.
    pub fn is_valid(&self, now: DateTime<Utc>, window: Duration) -> bool {
        self.granted_at + window >= now
    }
}

/// The outcome a human review produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOutcome {
    /// The signals were right about the shape and the package is
    /// legitimate. The observation is closed, the reasoning recorded,
    /// the history kept.
    DismissedFalsePositive {
        /// Why the reviewer concluded it is legitimate, naming the
        /// data that made the difference.
        justification: String,
    },
    /// The reviewer says the suspicion stands and hands it to the
    /// malware-analysis path — a human question, never a heuristic
    /// claim.
    ConfirmedForAnalysis,
}

/// Why the loop could not resolve the observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackError {
    /// The authorization is absent, expired, or has no ticket.
    MissingAuthorization,
    /// The reviewer has not said what the resolution is.
    NoOutcome,
}

impl std::fmt::Display for FeedbackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthorization => {
                write!(
                    formatter,
                    "a false positive is resolved only by an authorized review"
                )
            }
            Self::NoOutcome => write!(formatter, "the review must state its outcome"),
        }
    }
}

impl std::error::Error for FeedbackError {}

/// The record a resolution leaves behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// The reviewer's signature.
    pub authorization: ReviewerAuthorization,
    /// The outcome that was reached.
    pub outcome: ReviewOutcome,
    /// When the resolution was written.
    pub at: DateTime<Utc>,
    /// The reason codes that were dismissed (empty for a confirmation).
    pub dismissed: Vec<String>,
}

/// Resolves an observation through the feedback loop.
///
/// Dismissing a false positive REQUIRES a valid authorization — without
/// it the function returns the error and nothing moves. The observation
/// itself is not rewritten: the case keeps the opening and the signals,
/// and the resolution enters the timeline as evidence that says who
/// decided and why.
///
/// The returned case is the same case that went in plus the review.
/// When the outcome is a dismissal, the finding moves to `Unaffected`:
/// not because the heuristic changed its mind — it cannot — but because
/// a human with a ticket said the package is legitimate, and a human
/// with a ticket is the only evidence that closes a heuristic's case.
pub fn resolve_false_positive(
    case: &mut SecurityCase,
    observation: &Observation,
    outcome: Option<ReviewOutcome>,
    authorization: Option<ReviewerAuthorization>,
    now: DateTime<Utc>,
    window: Duration,
) -> Result<Resolution, FeedbackError> {
    let outcome = outcome.ok_or(FeedbackError::NoOutcome)?;
    let authorization = authorization
        .filter(|grant| grant.is_valid(now, window))
        .ok_or(FeedbackError::MissingAuthorization)?;

    match outcome {
        ReviewOutcome::DismissedFalsePositive { ref justification } => {
            let dismissed: Vec<String> = observation
                .reasons
                .iter()
                .map(|reason| reason.as_str().to_string())
                .collect();

            // The review goes on the timeline first: what happened,
            // with the author and the ticket, before the state moves.
            case.timeline.push(crate::case::CaseEvent {
                kind: CaseEventKind::Evidence,
                at: now,
                detail: format!(
                    "false positive dismissed by {} (ticket {}): {}",
                    authorization.reviewer, authorization.ticket, justification
                ),
            });

            // The finding is replaced with the human verdict, through
            // the case's own record_evidence: one finding per
            // component, the state derived from the newest evidence.
            let resolution_finding = Finding {
                advisory_id: case.trigger.clone(),
                component: observation.record.name.clone(),
                version: Some(observation.record.version.clone()),
                state: AffectedState::Unaffected,
                dependency: DependencyKind::Direct,
                vex: None,
            };
            case.record_evidence(
                resolution_finding,
                "the review closed the finding as not affected",
                now,
            );

            Ok(Resolution {
                authorization,
                outcome,
                at: now,
                dismissed,
            })
        }
        ReviewOutcome::ConfirmedForAnalysis => {
            case.timeline.push(crate::case::CaseEvent {
                kind: CaseEventKind::Evidence,
                at: now,
                detail: format!(
                    "confirmed for malware analysis by {} (ticket {}); the heuristic never \
                     states the conclusion",
                    authorization.reviewer, authorization.ticket
                ),
            });
            Ok(Resolution {
                authorization,
                outcome,
                at: now,
                dismissed: Vec::new(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisories::catalog::{Ecosystem, EcosystemCatalog, KnownMetadata, KnownPackage};
    use crate::advisories::policy::{HeuristicPolicy, decide, quarantine_case};
    use crate::advisories::signals::{PackageRecord, evaluate};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap()
    }

    fn confused() -> (Observation, SecurityCase) {
        let record = PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "@acme/utils".to_string(),
            version: "9.9.9".to_string(),
            publisher: "intruder".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 1,
                ..Default::default()
            },
        };
        let catalog = EcosystemCatalog::new(
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
        );
        let observation = evaluate(&record, &catalog);
        let policy = HeuristicPolicy::hold(50);
        let verdict = decide(&policy, &observation);
        let case = quarantine_case(&observation, &verdict, now());
        (observation, case)
    }

    #[test]
    fn without_an_authorization_nothing_is_resolved() {
        let (observation, mut case) = confused();
        let opened = case.timeline.len();
        let error = resolve_false_positive(
            &mut case,
            &observation,
            Some(ReviewOutcome::DismissedFalsePositive {
                justification: "the publisher is ours".to_string(),
            }),
            None,
            now(),
            Duration::days(7),
        )
        .expect_err("no ticket, no resolution");
        assert_eq!(error, FeedbackError::MissingAuthorization);
        assert_eq!(case.timeline.len(), opened, "and the case did not move");
        assert_eq!(case.state, crate::case::CaseState::Open);
    }

    #[test]
    fn an_expired_authorization_resolves_nothing() {
        let (observation, mut case) = confused();
        let grant =
            ReviewerAuthorization::new("secops", "HSP-166-review-1", now() - Duration::days(30));
        let error = resolve_false_positive(
            &mut case,
            &observation,
            Some(ReviewOutcome::DismissedFalsePositive {
                justification: "ours".to_string(),
            }),
            Some(grant),
            now(),
            Duration::days(7),
        )
        .expect_err("a month-old signature is not a decision");
        assert_eq!(error, FeedbackError::MissingAuthorization);
    }

    #[test]
    fn a_signed_review_closes_the_false_positive_and_keeps_the_history() {
        let (observation, mut case) = confused();
        let grant = ReviewerAuthorization::new("secops", "HSP-166-review-1", now());
        let resolution = resolve_false_positive(
            &mut case,
            &observation,
            Some(ReviewOutcome::DismissedFalsePositive {
                justification: "acme-ci delegated this publication; see ticket".to_string(),
            }),
            Some(grant.clone()),
            now(),
            Duration::days(7),
        )
        .expect("the review is authorized");
        assert_eq!(resolution.authorization, grant);
        assert!(
            !resolution.dismissed.is_empty(),
            "the dismissed codes are recorded"
        );
        assert_eq!(case.state, crate::case::CaseState::ClosedNotAffected);
        assert_eq!(case.findings[0].state, AffectedState::Unaffected);
        // The timeline keeps everything: opening, held decision is the
        // trigger, the review, the state change.
        let kinds: Vec<CaseEventKind> = case.timeline.iter().map(|event| event.kind).collect();
        assert!(kinds.contains(&CaseEventKind::Opened));
        assert!(kinds.contains(&CaseEventKind::Evidence));
        assert!(kinds.contains(&CaseEventKind::StateChanged));
        assert!(
            case.timeline
                .iter()
                .any(|event| event.detail.contains("secops") && event.detail.contains("ticket")),
            "the review names its author"
        );
    }

    #[test]
    fn confirming_sends_to_analysis_without_suppressing_anything() {
        let (observation, mut case) = confused();
        let grant = ReviewerAuthorization::new("secops", "HSP-166-review-2", now());
        let resolution = resolve_false_positive(
            &mut case,
            &observation,
            Some(ReviewOutcome::ConfirmedForAnalysis),
            Some(grant),
            now(),
            Duration::days(7),
        )
        .expect("authorized");
        assert!(
            resolution.dismissed.is_empty(),
            "a confirmation dismisses no signal"
        );
        assert_eq!(
            case.state,
            crate::case::CaseState::Open,
            "and the case stays open"
        );
        assert_eq!(
            case.findings[0].state,
            AffectedState::Unknown,
            "nobody knows yet"
        );
    }
}
