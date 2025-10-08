use crate::features::evaluate_policies::dto::{
    Decision, EvaluatePoliciesCommand, EvaluationDecision,
};
use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use crate::internal::engine::AuthorizationEngine;
use tracing::info;

/// Use case for evaluating authorization policies
///
/// This use case uses the authorization engine to evaluate policies against entities
/// and determine if access should be allowed or denied.
pub struct EvaluatePoliciesUseCase {
    /// Internal authorization engine
    engine: AuthorizationEngine,
}

impl Default for EvaluatePoliciesUseCase {
    fn default() -> Self {
        Self::new()
    }
}

impl EvaluatePoliciesUseCase {
    /// Create a new policy evaluation use case
    pub fn new() -> Self {
        Self {
            engine: AuthorizationEngine::new(),
        }
    }

    /// Execute policy evaluation
    ///
    /// This method evaluates an authorization request against loaded policies
    /// using the internal authorization engine.
    ///
    /// # Arguments
    ///
    /// * `command` - The evaluation command containing the request, policies, and entities
    ///
    /// # Returns
    ///
    /// An evaluation decision indicating whether access is allowed or denied
    ///
    /// # Errors
    ///
    /// Returns an error if policy evaluation fails due to:
    /// - Invalid policies
    /// - Translation errors
    /// - Cedar evaluation errors
    #[tracing::instrument(skip(self, command), fields(
        principal = %command.request.principal.hrn(),
        action = command.request.action,
        resource = %command.request.resource.hrn(),
        policy_count = command.policies.policies().len(),
        entity_count = command.entities.len()
    ))]
    pub async fn execute(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        info!(
            "Starting policy evaluation with {} policies and {} entities",
            command.policies.policies().len(),
            command.entities.len()
        );

        // 1. Load policies into the engine
        let policy_texts: Vec<String> = command
            .policies
            .policies()
            .iter()
            .map(|policy| policy.content().to_string())
            .collect();

        self.engine
            .load_policies(policy_texts)
            .await
            .map_err(|e| EvaluatePoliciesError::PolicyLoadError(e.to_string()))?;

        info!(
            "Successfully loaded {} policies",
            command.policies.policies().len()
        );

        // 2. Register entities in the engine
        self.engine
            .register_entities(command.entities.to_vec())
            .await
            .map_err(|e| EvaluatePoliciesError::EntityRegistrationError(e.to_string()))?;

        info!(
            "Successfully registered {} entities",
            command.entities.len()
        );

        // 3. Build engine request
        let engine_request = crate::internal::engine::types::EngineRequest::new(
            command.request.principal,
            command.request.action,
            command.request.resource,
        )
        .with_context(command.request.context.clone().unwrap_or_default());

        // 4. Evaluate authorization
        let decision = self
            .engine
            .is_authorized(&engine_request)
            .await
            .map_err(|e| EvaluatePoliciesError::EvaluationError(e.to_string()))?;

        info!("Policy evaluation completed successfully");

        // 5. Map engine decision to use case decision
        let mapped_decision = if decision.is_allowed() {
            Decision::Allow
        } else {
            Decision::Deny
        };

        // Simplified: no determining policies or reasons for now
        let determining_policies = vec![];
        let reasons = vec![];

        Ok(EvaluationDecision {
            decision: mapped_decision,
            determining_policies,
            reasons,
        })
    }

    /// Clear all cached data in the engine
    ///
    /// This method clears all loaded policies and registered entities,
    /// useful for testing or when you need to start fresh.
    pub async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError> {
        self.engine
            .clear_policies()
            .await
            .map_err(|e| EvaluatePoliciesError::CacheClearError(e.to_string()))?;

        self.engine
            .clear_entities()
            .await
            .map_err(|e| EvaluatePoliciesError::CacheClearError(e.to_string()))?;

        Ok(())
    }
}
