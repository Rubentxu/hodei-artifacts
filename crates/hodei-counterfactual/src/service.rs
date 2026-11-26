//! The service that composes the counterfactual answer.
//!
//! The path is short and fails closed:
//!
//! 1. First it **simulates**: if the hypothesis is not valid -- from another
//!    account, expired, against a world that no longer exists, or without a
//!    verifiable version --, it is rejected BEFORE anything is retrieved. No
//!    data would leak from the world the hypothesis claimed to touch, and a
//!    half answer about the wrong world is worse than no
//!    rejection.
//! 2. Then it **investigates**: the cause, with its citations and its seal of
//!    reads.
//! 3. The **shared seal** is checked: the sources that both
//!    manifests name have to be at the same version. If not,
//!    the investigation and the simulation do not speak of the same world and the
//!    answer is rejected as an incoherent seal.
//! 4. The report is composed: epistemic labels, references and the
//!    two inner contracts intact.
//!
//! And at no point is anything written: the simulation has nowhere to
//! write, and the investigation only reads. What is proposed stays
//! waiting for human approval in the investigator's guard.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hodei_investigator::investigator::{InvestigationError, Investigator};
use hodei_investigator::scope::RetrievalScope;
use hodei_simulation::service::SimulationService;
use hodei_simulation::{SimulationError, SimulationFork};

use crate::report::{CounterfactualReport, build_report, the_seal_coherence};

/// Why the hypothesis did not deserve an answer.
///
/// The reason is a CLASS, not a detail: rejection messages do not
/// carry accounts, HRNs or foreign identifiers, because rejecting a
/// hypothesis from another world is not an occasion to describe that other
/// world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// The hypothesis belongs to another account.
    OtherAccount,
    /// The hypothesis has lived longer than allowed.
    Expired,
    /// The hypothesis is larger than allowed.
    TooLarge,
    /// The world moved while it was being evaluated: the base no longer exists.
    StaleWorld,
    /// The base does not declare a verifiable version.
    UnverifiableBase,
    /// The world could not be read.
    Unavailable,
    /// The investigation and the simulation do not agree on the version of
    /// a shared source.
    IncoherentSeal,
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OtherAccount => write!(formatter, "the hypothesis belongs to another account"),
            Self::Expired => write!(formatter, "the hypothesis has expired"),
            Self::TooLarge => write!(formatter, "the hypothesis is too large"),
            Self::StaleWorld => write!(formatter, "the world of the hypothesis no longer exists"),
            Self::UnverifiableBase => {
                write!(
                    formatter,
                    "the base of the hypothesis declares no checkable version"
                )
            }
            Self::Unavailable => write!(formatter, "the world could not be read"),
            Self::IncoherentSeal => {
                write!(
                    formatter,
                    "the reads of the investigation and the base of the simulation do not agree on a shared source version"
                )
            }
        }
    }
}

/// Why there is no counterfactual answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterfactualError {
    /// The hypothesis was rejected, with its class and without its data.
    Rejected {
        /// The class of the rejection.
        reason: RejectionReason,
    },
    /// The investigation could not answer: its own error
    /// says so, with the scope of whoever asks.
    Investigation(InvestigationError),
}

impl std::fmt::Display for CounterfactualError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected { reason } => write!(formatter, "the hypothesis is rejected: {reason}"),
            Self::Investigation(error) => {
                write!(formatter, "the investigation could not answer: {error}")
            }
        }
    }
}

impl std::error::Error for CounterfactualError {}

/// The counterfactual investigation service.
pub struct CounterfactualService {
    investigator: Arc<Investigator>,
    simulation: Arc<SimulationService>,
}

impl CounterfactualService {
    /// The service over an investigator and a simulator.
    pub fn new(investigator: Arc<Investigator>, simulation: Arc<SimulationService>) -> Self {
        Self {
            investigator,
            simulation,
        }
    }

    /// Answers the question under the fork's hypothesis.
    ///
    /// The `now` is the same clock for both halves: it is what makes
    /// the seal of one answer comparable with that of the
    /// other, and what makes it reproducible.
    pub async fn answer(
        &self,
        counterfactual: &str,
        scope: &RetrievalScope,
        question: &str,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<CounterfactualReport, CounterfactualError> {
        // 1. The hypothesis first: a rejection spends no reads.
        let simulation = self
            .simulation
            .simulate_result(fork, now)
            .await
            .map_err(the_rejection)?;

        // 2. Then the cause.
        let investigation = self
            .investigator
            .investigate(counterfactual, scope, question, now)
            .await
            .map_err(CounterfactualError::Investigation)?;

        // 3. And the two have to speak of the same world.
        let seal = the_seal_coherence(&investigation.report.read_set, &simulation.base.sources);
        if !seal.coherent {
            return Err(CounterfactualError::Rejected {
                reason: RejectionReason::IncoherentSeal,
            });
        }

        // 4. Compose, without deciding.
        Ok(build_report(
            counterfactual,
            question,
            &investigation.report,
            &simulation,
            seal,
        ))
    }
}

/// The rejection of a simulation failure: the class, never the detail.
///
/// The `SimulationError` messages carry accounts and identities for
/// whoever simulates; here the detail is swapped for the class because whoever
/// receives the rejection has no reason to see another account's world.
fn the_rejection(error: SimulationError) -> CounterfactualError {
    let reason = match error {
        SimulationError::NotAllowed(_) => RejectionReason::OtherAccount,
        SimulationError::Expired(_) => RejectionReason::Expired,
        SimulationError::TooLarge(_) => RejectionReason::TooLarge,
        SimulationError::StaleBase { .. } => RejectionReason::StaleWorld,
        SimulationError::NoVersion(_) => RejectionReason::UnverifiableBase,
        SimulationError::Unavailable(_) | SimulationError::ReadFailed(_) => {
            RejectionReason::Unavailable
        }
    };
    CounterfactualError::Rejected { reason }
}
