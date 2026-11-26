//! The capability guard: what the investigator can do and what
//! needs approval.
//!
//! The investigator **reads**. Reading needs no permission: it is what it has been
//! asked to do. But anything else (opening a PR, touching a deployment,
//! re-evaluating a decision) **needs the approval of an authorized
//! person**, and the approval is left in the trace.
//!
//! And while there is no approval, the action **remains pending**: it is not
//! executed, not scheduled and not forgotten. An action waiting for an
//! approval that nobody sees is an action that runs by itself one day.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What the investigator wants to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedAction {
    /// What kind of action it is.
    pub kind: ActionKind,
    /// What the action says, in words.
    pub description: String,
    /// Whose decision the action would take.
    pub subject: String,
    /// Who requests it.
    pub requested_by: String,
    /// When it is requested.
    pub requested_at: DateTime<Utc>,
}

/// What kind of action it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Read. Needs no approval.
    Read,
    /// Write to the graph or to the decisions. Needs approval.
    Mutate,
    /// Run something. Needs approval.
    Execute,
    /// Propose a promotion. Needs approval.
    Promote,
}

/// What an action can do.
impl ActionKind {
    /// Whether the action needs approval.
    ///
    /// Reading does not: it is what was asked for. Everything else does.
    pub fn needs_approval(&self) -> bool {
        !matches!(self, Self::Read)
    }
}

/// The state of a proposed action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionState {
    /// Waiting for an approval.
    Pending {
        /// Who can approve.
        approver: String,
    },
    /// Approved.
    Approved {
        /// Who approved it.
        by: String,
        /// When.
        at: DateTime<Utc>,
    },
    /// Denied.
    Denied {
        /// Who denied it.
        by: String,
        /// When.
        at: DateTime<Utc>,
        /// Why.
        reason: String,
    },
}

/// A proposed action and its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracedAction {
    /// The action.
    pub action: ProposedAction,
    /// Its state.
    pub state: ActionState,
}

impl TracedAction {
    /// An action awaiting approval.
    pub fn pending(action: ProposedAction, approver: impl Into<String>) -> Self {
        Self {
            action,
            state: ActionState::Pending {
                approver: approver.into(),
            },
        }
    }

    /// The action keeps waiting.
    pub fn is_pending(&self) -> bool {
        matches!(self.state, ActionState::Pending { .. })
    }

    /// The action is approved.
    pub fn is_approved(&self) -> bool {
        matches!(self.state, ActionState::Approved { .. })
    }
}

/// The capability guard.
///
/// The guard does **not** execute anything: it records what is requested and makes clear
/// that an approval is needed. And the approval is given by a person, with
/// their name and their moment, and that is left in the trace.
#[derive(Debug, Clone, Default)]
pub struct CapabilityGuard {
    actions: Vec<TracedAction>,
    approvers: Vec<String>,
}

impl CapabilityGuard {
    /// A guard without approvers: nobody can approve anything.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A guard with those who can approve.
    pub fn with_approvers(approvers: Vec<String>) -> Self {
        Self {
            actions: Vec::new(),
            approvers,
        }
    }

    /// Records a proposed action.
    ///
    /// A read action is **approved** from the start, because reading is
    /// what it has been asked to do. Everything else stays **pending**.
    pub fn propose(&mut self, action: ProposedAction) -> &TracedAction {
        if action.kind.needs_approval() {
            let approver = self
                .approvers
                .first()
                .cloned()
                .unwrap_or_else(|| "nadie".to_string());
            self.actions.push(TracedAction::pending(action, approver));
        } else {
            self.actions.push(TracedAction {
                action,
                state: ActionState::Approved {
                    by: "la propia peticion".to_string(),
                    at: Utc::now(),
                },
            });
        }
        self.actions.last().expect("la accion se acaba de anadir")
    }

    /// Approves an action, if the approver can.
    pub fn approve(&mut self, index: usize, by: &str, at: DateTime<Utc>) -> Result<(), GuardError> {
        let approver = self
            .approvers
            .iter()
            .find(|one| one.as_str() == by)
            .ok_or(GuardError::NotAnApprover(by.to_string()))?;
        let action = self
            .actions
            .get_mut(index)
            .ok_or(GuardError::NoSuchAction(index))?;
        if !action.is_pending() {
            return Err(GuardError::NotPending);
        }
        action.state = ActionState::Approved {
            by: approver.clone(),
            at,
        };
        Ok(())
    }

    /// Denies an action.
    pub fn deny(
        &mut self,
        index: usize,
        by: &str,
        at: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Result<(), GuardError> {
        let approver = self
            .approvers
            .iter()
            .find(|one| one.as_str() == by)
            .ok_or(GuardError::NotAnApprover(by.to_string()))?;
        let action = self
            .actions
            .get_mut(index)
            .ok_or(GuardError::NoSuchAction(index))?;
        if !action.is_pending() {
            return Err(GuardError::NotPending);
        }
        action.state = ActionState::Denied {
            by: approver.clone(),
            at,
            reason: reason.into(),
        };
        Ok(())
    }

    /// The recorded actions.
    pub fn actions(&self) -> &[TracedAction] {
        &self.actions
    }

    /// Those that are still waiting.
    pub fn pending(&self) -> Vec<&TracedAction> {
        self.actions
            .iter()
            .filter(|action| action.is_pending())
            .collect()
    }
}

/// Why the guard could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The approver cannot approve.
    NotAnApprover(String),
    /// There is no such action.
    NoSuchAction(usize),
    /// The action was not waiting.
    NotPending,
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnApprover(name) => {
                write!(formatter, "{name} is not an approver")
            }
            Self::NoSuchAction(index) => write!(formatter, "there is no action {index}"),
            Self::NotPending => write!(formatter, "the action is not pending"),
        }
    }
}
