//! The events that trigger the reactions.
//!
//! An event is a fact that happened: a vulnerability was accepted, an
//! exception expired, a piece of evidence arrived. It carries an
//! identifier, and that identifier is what makes the reaction
//! **once-only**: the same event delivered twice is the same event, and
//! the second delivery is suppressed instead of repeating the effect.
//!
//! The subscription says what an event has to look like for a behaviour
//! to be interested in it. The predicate is a small, **closed** set of
//! checks and not a language: a behaviour that decide sobre el payload
//! con reglas libres seria un motor de workflows, y un motor de workflows
//! es mas de lo que hace falta para reaccionar a un CVE.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An event that happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// The identifier of the event.
    ///
    /// It is what makes the reaction once-only: the same identifier is
    /// the same event, delivered again.
    pub id: String,
    /// The kind of the event.
    pub kind: EventKind,
    /// What the event carries.
    pub payload: serde_json::Value,
    /// When it happened.
    pub happened_at: DateTime<Utc>,
    /// The event that caused this one, if there was one.
    ///
    /// The chain of causation is what lets a reader see WHY a reaction
    /// happened and not only that it happened.
    pub caused_by: Option<String>,
}

/// The kinds of event the runtime knows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A vulnerability was accepted.
    VulnerabilityAccepted,
    /// An exception expired.
    ExceptionExpired,
    /// A piece of evidence arrived.
    EvidenceAccepted,
    /// A decision was taken.
    DecisionTaken,
}

impl EventKind {
    /// The name of the kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VulnerabilityAccepted => "vulnerability_accepted",
            Self::ExceptionExpired => "exception_expired",
            Self::EvidenceAccepted => "evidence_accepted",
            Self::DecisionTaken => "decision_taken",
        }
    }
}

impl Event {
    /// An event of a kind.
    pub fn new(
        id: impl Into<String>,
        kind: EventKind,
        payload: serde_json::Value,
        happened_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            payload,
            happened_at,
            caused_by: None,
        }
    }

    /// The event was caused by another one.
    pub fn caused_by(mut self, event_id: impl Into<String>) -> Self {
        self.caused_by = Some(event_id.into());
        self
    }
}

/// What an event has to look like for a behaviour to be interested in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    /// The kind of the event.
    pub kind: EventKind,
    /// What the payload has to satisfy.
    pub predicate: Option<PayloadPredicate>,
}

impl Subscription {
    /// A subscription to a kind of event.
    pub fn to(kind: EventKind) -> Self {
        Self {
            kind,
            predicate: None,
        }
    }

    /// The subscription also asks something of the payload.
    pub fn where_(mut self, predicate: PayloadPredicate) -> Self {
        self.predicate = Some(predicate);
        self
    }

    /// Whether the event matches the subscription.
    pub fn matches(&self, event: &Event) -> bool {
        if self.kind != event.kind {
            return false;
        }
        match &self.predicate {
            Some(predicate) => predicate.holds(&event.payload),
            None => true,
        }
    }
}

/// What the payload of an event has to satisfy.
///
/// The set is closed on purpose. A predicate that could say anything
/// would be a query language, and a query language here would be a
/// workflow engine wearing another name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadPredicate {
    /// The field is there and it is equal to the value.
    FieldEquals {
        /// The name of the field.
        field: String,
        /// The value it has to have.
        value: serde_json::Value,
    },
    /// The field is there.
    FieldExists {
        /// The name of the field.
        field: String,
    },
    /// The field is NOT there.
    FieldAbsent {
        /// The name of the field.
        field: String,
    },
}

impl PayloadPredicate {
    /// Whether the payload satisfies the predicate.
    pub fn holds(&self, payload: &serde_json::Value) -> bool {
        match self {
            Self::FieldEquals { field, value } => payload.get(field) == Some(value),
            Self::FieldExists { field } => payload.get(field).is_some(),
            Self::FieldAbsent { field } => payload.get(field).is_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn the_payload() -> serde_json::Value {
        serde_json::json!({"advisory": "GHSA-1", "account": "acme"})
    }

    #[test]
    fn a_subscription_matches_the_kind_and_the_payload() {
        let event = Event::new(
            "ev-1",
            EventKind::VulnerabilityAccepted,
            the_payload(),
            Utc::now(),
        );

        assert!(Subscription::to(EventKind::VulnerabilityAccepted).matches(&event));
        assert!(!Subscription::to(EventKind::ExceptionExpired).matches(&event));

        let subscription = Subscription::to(EventKind::VulnerabilityAccepted).where_(
            PayloadPredicate::FieldEquals {
                field: "advisory".to_string(),
                value: serde_json::json!("GHSA-1"),
            },
        );
        assert!(subscription.matches(&event));

        let other = Subscription::to(EventKind::VulnerabilityAccepted).where_(
            PayloadPredicate::FieldEquals {
                field: "advisory".to_string(),
                value: serde_json::json!("GHSA-2"),
            },
        );
        assert!(!other.matches(&event));
    }
}
