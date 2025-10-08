//! Mocks for the evaluate_policies feature
//!
//! This module provides mock implementations of the ports for testing.

use super::dto::{Decision, EvaluatePoliciesCommand, EvaluationDecision};
use super::error::EvaluatePoliciesError;
use super::ports::EvaluatePoliciesPort;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Mock implementation of EvaluatePoliciesPort for testing
///
/// This mock allows tests to verify interactions with the policy evaluator
/// without requiring the full Cedar engine setup.
#[derive(Default)]
pub struct MockEvaluatePoliciesPort {
    /// Pre-configured decision to return
    decision: Arc<Mutex<Decision>>,
    /// Track number of times evaluate was called
    evaluate_call_count: Arc<Mutex<usize>>,
    /// Track number of times clear_cache was called
    clear_cache_call_count: Arc<Mutex<usize>>,
    /// Optional error to return
    error: Arc<Mutex<Option<EvaluatePoliciesError>>>,
}

impl MockEvaluatePoliciesPort {
    /// Create a new mock that returns the specified decision
    pub fn new_with_decision(decision: Decision) -> Self {
        Self {
            decision: Arc::new(Mutex::new(decision)),
            evaluate_call_count: Arc::new(Mutex::new(0)),
            clear_cache_call_count: Arc::new(Mutex::new(0)),
            error: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new mock that returns an error
    pub fn new_with_error(error: EvaluatePoliciesError) -> Self {
        Self {
            decision: Arc::new(Mutex::new(Decision::Deny)),
            evaluate_call_count: Arc::new(Mutex::new(0)),
            clear_cache_call_count: Arc::new(Mutex::new(0)),
            error: Arc::new(Mutex::new(Some(error))),
        }
    }

    /// Set the decision to return
    pub fn set_decision(&self, decision: Decision) {
        *self.decision.lock().unwrap() = decision;
    }

    /// Set an error to return
    pub fn set_error(&self, error: Option<EvaluatePoliciesError>) {
        *self.error.lock().unwrap() = error;
    }

    /// Get the number of times evaluate was called
    pub fn evaluate_call_count(&self) -> usize {
        *self.evaluate_call_count.lock().unwrap()
    }

    /// Get the number of times clear_cache was called
    pub fn clear_cache_call_count(&self) -> usize {
        *self.clear_cache_call_count.lock().unwrap()
    }

    /// Reset all counters
    pub fn reset(&self) {
        *self.evaluate_call_count.lock().unwrap() = 0;
        *self.clear_cache_call_count.lock().unwrap() = 0;
    }
}

#[async_trait]
impl EvaluatePoliciesPort for MockEvaluatePoliciesPort {
    async fn evaluate(
        &self,
        _command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        *self.evaluate_call_count.lock().unwrap() += 1;

        if let Some(error) = self.error.lock().unwrap().as_ref() {
            return Err(EvaluatePoliciesError::InternalError(error.to_string()));
        }

        let decision = self.decision.lock().unwrap().clone();
        Ok(EvaluationDecision {
            decision,
            determining_policies: vec![],
            reasons: vec![],
            used_schema_version: None,
            policy_ids_evaluated: vec![],
            diagnostics: vec![],
        })
    }

    async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError> {
        *self.clear_cache_call_count.lock().unwrap() += 1;

        if let Some(error) = self.error.lock().unwrap().as_ref() {
            return Err(EvaluatePoliciesError::InternalError(error.to_string()));
        }

        Ok(())
    }
}
