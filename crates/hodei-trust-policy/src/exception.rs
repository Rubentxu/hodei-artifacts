//! The lifecycle of an exception.
//!
//! An exception is not a switch: it is a thing that somebody asked for,
//! that somebody else approved, and that stops working at a moment that
//! was written down when it was approved. The three parts are not
//! decoration -- they are what makes an exception different from a hole in
//! the policy.
//!
//! The expiry is **mandatory**. An exception without an expiry is not
//! approved, and the rule that says so is in the code and not only in the
//! documentation, because the first exception without an expiry is the one
//! that lasts forever.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::reasons::ReasonCode;

/// The state of an exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExceptionState {
    /// Somebody asked for it and nobody has answered yet.
    Requested,
    /// Somebody approved it and it has not expired.
    Approved,
    /// Somebody approved it and it ran out.
    Expired,
    /// Somebody refused it.
    Refused,
}

/// A request for an exception.
///
/// It names what it waives, why it asks for it, who asks and who approves.
/// Without the approver and without the expiry it is not approvable, and
/// the code says so rather than letting a request through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionRequest {
    /// The identifier of the request.
    pub id: String,
    /// The name the policy gives the exception.
    pub name: String,
    /// The subject the request is about.
    pub subject: String,
    /// The requirement it asks to waive.
    pub overrides: String,
    /// Why it asks for it.
    pub reason: String,
    /// Who asks for it.
    pub requested_by: String,
    /// Who approves it.
    pub approved_by: Option<String>,
    /// When it stops working.
    pub expires_at: Option<DateTime<Utc>>,
    /// The compensating controls that go with it.
    pub compensating_controls: Vec<String>,
    /// The ticket the request is attached to.
    pub ticket: Option<String>,
    /// The state of the request.
    pub state: ExceptionState,
}

/// Why an exception request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    /// It did not say when it stops working.
    NoExpiry,
    /// It did not say who approves it.
    NoApprover,
    /// The approver is the one that asked for it.
    SelfApproval,
}

/// The outcome of trying to approve an exception request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// It was approved.
    Approved,
    /// It was refused, and this is why.
    Refused(RefusalReason),
}

impl ExceptionRequest {
    /// A request for an exception.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        subject: impl Into<String>,
        overrides: impl Into<String>,
        reason: impl Into<String>,
        requested_by: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            subject: subject.into(),
            overrides: overrides.into(),
            reason: reason.into(),
            requested_by: requested_by.into(),
            approved_by: None,
            expires_at: None,
            compensating_controls: Vec::new(),
            ticket: None,
            state: ExceptionState::Requested,
        }
    }

    /// The controls that go with it.
    pub fn with_controls(mut self, controls: Vec<String>) -> Self {
        self.compensating_controls = controls;
        self
    }

    /// The ticket it is attached to.
    pub fn with_ticket(mut self, ticket: impl Into<String>) -> Self {
        self.ticket = Some(ticket.into());
        self
    }

    /// Approves the request, and says when it stops working.
    ///
    /// The expiry is not optional and the approver cannot be the one that
    /// asked for it: an exception that nobody else agreed to is not an
    /// exception, it is a hole.
    pub fn approve(
        &mut self,
        approved_by: impl Into<String>,
        expires_at: DateTime<Utc>,
    ) -> ApprovalOutcome {
        let approved_by = approved_by.into();
        if expires_at <= Utc::now() {
            self.state = ExceptionState::Expired;
            return ApprovalOutcome::Refused(RefusalReason::NoExpiry);
        }
        if approved_by == self.requested_by {
            self.state = ExceptionState::Refused;
            return ApprovalOutcome::Refused(RefusalReason::SelfApproval);
        }
        self.approved_by = Some(approved_by);
        self.expires_at = Some(expires_at);
        self.state = ExceptionState::Approved;
        ApprovalOutcome::Approved
    }

    /// Refuses the request.
    pub fn refuse(mut self) -> Self {
        self.state = ExceptionState::Refused;
        self
    }

    /// Whether the exception is usable at a moment.
    ///
    /// An exception that expired is not usable, and it cannot be reused:
    /// the decision that leans on it says that it was active at the moment
    /// it was made, and that moment is now.
    pub fn is_usable_at(&self, now: DateTime<Utc>) -> bool {
        self.state == ExceptionState::Approved
            && self.expires_at.is_some_and(|expires_at| expires_at > now)
    }

    /// The fact that the evaluation reads.
    pub fn to_fact(
        &self,
        subject: &str,
        now: DateTime<Utc>,
    ) -> Option<crate::context::ExceptionFact> {
        if !self.is_usable_at(now) {
            return None;
        }
        Some(crate::context::ExceptionFact {
            id: self.id.clone(),
            name: self.name.clone(),
            expires_at: self.expires_at.expect("an approved exception expires"),
            applies_to_subject: self.subject == subject,
        })
    }

    /// The reason why the exception cannot be used, if it cannot.
    pub fn refusal(&self, now: DateTime<Utc>) -> Option<ReasonCode> {
        match self.state {
            ExceptionState::Requested => Some(ReasonCode::ExceptionRequired),
            ExceptionState::Refused => Some(ReasonCode::ExceptionRequired),
            ExceptionState::Expired => Some(ReasonCode::ExceptionRequired),
            ExceptionState::Approved => {
                if self.expires_at.is_some_and(|expires_at| expires_at <= now) {
                    Some(ReasonCode::ExceptionRequired)
                } else {
                    None
                }
            }
        }
    }
}

/// The default life of an approved exception.
///
/// It is a default and not a constant: a deployment may want another, and
/// the request always says when it stops working.
pub const DEFAULT_EXCEPTION_LIFE: Duration = Duration::days(30);

#[cfg(test)]
mod tests {
    use super::*;

    fn a_request() -> ExceptionRequest {
        ExceptionRequest::new(
            "ex-1",
            "sbom-waiver",
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0",
            "dimension:inventory_completeness",
            "the scan is not finished",
            "alice",
        )
    }

    #[test]
    fn an_exception_without_an_expiry_is_not_approved() {
        let outcome = a_request().approve("bob", Utc::now() - Duration::hours(1));
        assert_eq!(outcome, ApprovalOutcome::Refused(RefusalReason::NoExpiry));
    }

    #[test]
    fn nobody_approves_their_own_exception() {
        let outcome = a_request().approve("alice", Utc::now() + Duration::days(1));
        assert_eq!(
            outcome,
            ApprovalOutcome::Refused(RefusalReason::SelfApproval)
        );
    }

    #[test]
    fn an_approved_exception_stops_working_when_it_expires() {
        let now = Utc::now();
        let mut exception = a_request();
        assert_eq!(
            exception.approve("bob", now + Duration::hours(1)),
            ApprovalOutcome::Approved
        );
        assert!(exception.is_usable_at(now));
        assert!(
            !exception.is_usable_at(now + Duration::hours(2)),
            "an exception that ran out is not usable"
        );
    }

    #[test]
    fn an_exception_that_was_never_approved_is_not_usable() {
        let exception = a_request();
        assert!(!exception.is_usable_at(Utc::now()));
        assert_eq!(
            exception.refusal(Utc::now()),
            Some(ReasonCode::ExceptionRequired)
        );
    }
}
