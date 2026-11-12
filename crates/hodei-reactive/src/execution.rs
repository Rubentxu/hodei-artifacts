//! The execution record, the budget and the store.
//!
//! The record is **durable** and it is the unit of once-only: the
//! runtime looks for a record with the same key before it does anything,
//! and a record that is there means the reaction already happened.
//!
//! The budget is what keeps a reaction from looping. A system that
//! reevaluates can reevaluate because of the reevaluation, and a loop
//! that fires effects is an incident. The budget cuts the chain, and **the
//! cut is in the trace**: a reaction that stopped without saying why is
//! indistinguishable from a reaction that finished.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::behavior::TraceEntry;

/// How an execution ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    /// It ran and it did what it had to do.
    Completed,
    /// It did not run, and this is why.
    Skipped {
        /// The reason.
        reason: String,
    },
    /// It ran and it failed.
    Failed {
        /// What went wrong.
        reason: String,
        /// Whether trying again can work.
        retryable: bool,
    },
    /// It did not run because it had already run.
    ///
    /// The delivery was a repetition, and the effect of the first
    /// delivery stands.
    Duplicate,
    /// It was cut by the budget.
    Cut {
        /// Why it was cut.
        reason: String,
    },
}

/// One execution of one behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// The key that makes the execution once-only.
    pub key: String,
    /// The behaviour that ran.
    pub behavior: String,
    /// The event it ran for.
    pub event_id: String,
    /// The chain of events that led here.
    ///
    /// The first one is the event that started the chain, and the last one
    /// is the event this execution ran for.
    pub causation: Vec<String>,
    /// How many times it was attempted.
    pub attempts: u32,
    /// How it ended.
    pub outcome: ExecutionOutcome,
    /// The trace of the execution.
    pub trace: Vec<TraceEntry>,
    /// When it started.
    pub started_at: DateTime<Utc>,
    /// When it finished.
    pub finished_at: Option<DateTime<Utc>>,
}

impl ExecutionRecord {
    /// An execution that has not finished.
    pub fn new(
        key: impl Into<String>,
        behavior: impl Into<String>,
        event_id: impl Into<String>,
        causation: Vec<String>,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            key: key.into(),
            behavior: behavior.into(),
            event_id: event_id.into(),
            causation,
            attempts: 0,
            outcome: ExecutionOutcome::Completed,
            trace: Vec::new(),
            started_at,
            finished_at: None,
        }
    }

    /// The execution has an attempt.
    pub fn with_attempt(mut self) -> Self {
        self.attempts += 1;
        self
    }

    /// The execution says something.
    pub fn note(mut self, what: impl Into<String>, at: DateTime<Utc>) -> Self {
        self.trace.push(TraceEntry {
            what: what.into(),
            at,
        });
        self
    }

    /// The execution ended.
    pub fn finished(mut self, outcome: ExecutionOutcome, at: DateTime<Utc>) -> Self {
        self.outcome = outcome;
        self.finished_at = Some(at);
        self
    }

    /// Whether the execution failed with a failure that can be retried.
    pub fn is_retryable_failure(&self) -> bool {
        matches!(
            self.outcome,
            ExecutionOutcome::Failed {
                retryable: true,
                ..
            }
        )
    }

    /// Whether the execution failed at all.
    pub fn is_failure(&self) -> bool {
        matches!(self.outcome, ExecutionOutcome::Failed { .. })
    }
}

/// How much the runtime is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// How many executions are allowed in a chain.
    pub max_executions: usize,
    /// How deep the chain may be.
    pub max_depth: usize,
    /// How many times an event may be attempted.
    pub max_attempts: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_executions: 32,
            max_depth: 8,
            max_attempts: 3,
        }
    }
}

/// The state of the budget while a chain runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetState {
    /// The budget.
    pub budget: Budget,
    /// How many executions have happened.
    pub executions: usize,
    /// How deep the chain is.
    pub depth: usize,
}

impl BudgetState {
    /// The state of a budget.
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            executions: 0,
            depth: 0,
        }
    }

    /// Whether the budget has run out.
    ///
    /// It runs out when there have been too many executions, when the
    /// chain is too deep, or when an event has been attempted too many
    /// times.
    pub fn is_exhausted(&self, attempts: u32) -> Option<String> {
        if self.executions >= self.budget.max_executions {
            return Some(format!(
                "the chain ran {} executions and the budget allows {}",
                self.executions, self.budget.max_executions
            ));
        }
        if self.depth >= self.budget.max_depth {
            return Some(format!(
                "the chain is {} deep and the budget allows {}",
                self.depth, self.budget.max_depth
            ));
        }
        if attempts >= self.budget.max_attempts {
            return Some(format!(
                "the event was attempted {} times and the budget allows {}",
                attempts, self.budget.max_attempts
            ));
        }
        None
    }

    /// The budget consumes one execution.
    pub fn consume(&mut self) {
        self.executions += 1;
    }

    /// The chain goes one level deeper.
    pub fn deepen(&mut self) {
        self.depth += 1;
    }

    /// The chain goes one level back.
    pub fn shallow(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_cuts_when_it_is_exhausted() {
        let mut state = BudgetState::new(Budget {
            max_executions: 1,
            max_depth: 8,
            max_attempts: 3,
        });
        assert!(state.is_exhausted(0).is_none());
        state.consume();
        assert!(
            state.is_exhausted(0).is_some(),
            "the budget says why it cut"
        );
    }

    #[test]
    fn a_deep_chain_is_cut() {
        let mut state = BudgetState::new(Budget {
            max_executions: 32,
            max_depth: 2,
            max_attempts: 3,
        });
        state.deepen();
        state.deepen();
        assert!(state.is_exhausted(0).is_some());
    }
}
