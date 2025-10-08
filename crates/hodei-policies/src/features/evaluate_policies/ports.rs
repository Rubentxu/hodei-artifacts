//! Ports (trait definitions) for the evaluate_policies feature
//!
//! This module defines the public interfaces that the EvaluatePoliciesUseCase
//! depends on. These traits enable dependency inversion and testability.

use async_trait::async_trait;

use crate::features::evaluate_policies::dto::{EvaluatePoliciesCommand, EvaluationDecision};
use crate::features::evaluate_policies::error::EvaluatePoliciesError;

/// Port for policy evaluation operations
///
/// This trait defines the contract for policy evaluation functionality.
/// It allows other bounded contexts to depend on policy evaluation
/// without coupling to the concrete implementation.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::evaluate_policies::ports::EvaluatePoliciesPort;
///
/// async fn authorize_request(
///     evaluator: &dyn EvaluatePoliciesPort,
///     command: EvaluatePoliciesCommand<'_>
/// ) -> Result<bool, EvaluatePoliciesError> {
///     let decision = evaluator.evaluate(command).await?;
///     Ok(decision.decision == Decision::Allow)
/// }
/// ```
#[async_trait]
pub trait EvaluatePoliciesPort: Send + Sync {
    /// Evaluate authorization policies against a request
    ///
    /// This method evaluates whether a principal is authorized to perform
    /// an action on a resource, given a set of policies and entity context.
    ///
    /// # Arguments
    ///
    /// * `command` - The evaluation command containing the request, policies, and entities
    ///
    /// # Returns
    ///
    /// An evaluation decision indicating whether access is allowed or denied,
    /// along with determining policies and reasons.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Policy loading fails
    /// - Entity registration fails
    /// - Policy evaluation encounters an error
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError>;

    /// Clear all cached policies and entities
    ///
    /// This method clears the internal cache of the policy evaluator,
    /// removing all loaded policies and registered entities.
    /// Useful for testing or when you need to refresh the evaluation context.
    ///
    /// # Errors
    ///
    /// Returns an error if cache clearing fails
    async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError>;
}
