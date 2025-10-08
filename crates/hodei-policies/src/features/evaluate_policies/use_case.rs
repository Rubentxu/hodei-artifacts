use crate::features::build_schema::ports::SchemaStoragePort;
use crate::features::evaluate_policies::dto::{
    Decision, DiagnosticLevel, EvaluatePoliciesCommand, EvaluationDecision, EvaluationMode,
};
use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use crate::features::evaluate_policies::ports::EvaluatePoliciesPort;
use crate::internal::engine::AuthorizationEngine;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Use case for evaluating authorization policies
///
/// This use case uses the authorization engine to evaluate policies against entities
/// and determine if access should be allowed or denied.
///
/// It supports schema-aware evaluation by loading Cedar schemas from storage,
/// with configurable fallback behavior when schemas are not found.
pub struct EvaluatePoliciesUseCase {
    /// Internal authorization engine
    engine: AuthorizationEngine,

    /// Schema storage port for loading schemas
    schema_storage: Arc<dyn SchemaStoragePort>,
}

impl EvaluatePoliciesUseCase {
    /// Create a new policy evaluation use case
    ///
    /// # Arguments
    ///
    /// * `schema_storage` - Port implementation for loading schemas from storage
    pub fn new(schema_storage: Arc<dyn SchemaStoragePort>) -> Self {
        Self {
            engine: AuthorizationEngine::new(),
            schema_storage,
        }
    }

    /// Execute policy evaluation
    ///
    /// This method evaluates an authorization request against loaded policies
    /// using the internal authorization engine.
    ///
    /// The evaluation process follows these steps:
    /// 1. Optionally load a Cedar schema based on the evaluation mode
    /// 2. Load policies into the engine
    /// 3. Register entities in the engine
    /// 4. Build the authorization request
    /// 5. Evaluate and return the decision
    ///
    /// # Arguments
    ///
    /// * `command` - The evaluation command containing the request, policies, entities,
    ///              schema version preference, and evaluation mode
    ///
    /// # Returns
    ///
    /// An evaluation decision indicating whether access is allowed or denied,
    /// along with diagnostic information and the schema version used (if any).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema loading fails in Strict mode
    /// - Invalid policies
    /// - Translation errors
    /// - Cedar evaluation errors
    #[tracing::instrument(skip(self, command), fields(
        principal = %command.request.principal.hrn(),
        action = command.request.action,
        resource = %command.request.resource.hrn(),
        policy_count = command.policies.policies().len(),
        entity_count = command.entities.len(),
        schema_version = ?command.schema_version,
        evaluation_mode = ?command.evaluation_mode
    ))]
    pub async fn execute(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        info!(
            "Starting policy evaluation with {} policies and {} entities, mode: {:?}",
            command.policies.policies().len(),
            command.entities.len(),
            command.evaluation_mode
        );

        // Step 1: Load schema based on evaluation mode
        let schema_result = self.load_schema_for_evaluation(&command).await;
        let (used_schema_version, diagnostics) = match schema_result {
            Ok((version, diags)) => (version, diags),
            Err(e) => {
                // In Strict mode, schema load failures are fatal
                if command.evaluation_mode == EvaluationMode::Strict {
                    return Err(e);
                }
                // In other modes, log and continue
                warn!(
                    "Schema loading failed but continuing in non-strict mode: {}",
                    e
                );
                (None, vec![])
            }
        };

        // Step 2: Load policies into the engine
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

        // Step 3: Register entities in the engine
        self.engine
            .register_entities(command.entities.to_vec())
            .await
            .map_err(|e| EvaluatePoliciesError::EntityRegistrationError(e.to_string()))?;

        info!(
            "Successfully registered {} entities",
            command.entities.len()
        );

        // Step 4: Build engine request
        let engine_request = crate::internal::engine::types::EngineRequest::new(
            command.request.principal,
            command.request.action,
            command.request.resource,
        )
        .with_context(command.request.context.clone().unwrap_or_default());

        // Step 5: Evaluate authorization
        let decision = self
            .engine
            .is_authorized(&engine_request)
            .await
            .map_err(|e| EvaluatePoliciesError::EvaluationError(e.to_string()))?;

        debug!(
            decision = decision.is_allowed(),
            "Policy evaluation completed"
        );

        // Step 6: Map engine decision to use case decision
        let mapped_decision = if decision.is_allowed() {
            Decision::Allow
        } else {
            Decision::Deny
        };

        // Collect policy IDs that were evaluated
        let policy_ids_evaluated: Vec<String> = command
            .policies
            .policies()
            .iter()
            .map(|p| p.id().to_string())
            .collect();

        info!(
            decision = ?mapped_decision,
            schema_version = ?used_schema_version,
            "Policy evaluation completed successfully"
        );

        // Step 7: Build and return evaluation decision
        let mut evaluation_decision = EvaluationDecision {
            decision: mapped_decision,
            determining_policies: vec![],
            reasons: vec![],
            used_schema_version,
            policy_ids_evaluated,
            diagnostics,
        };

        // Add success diagnostic
        evaluation_decision.diagnostics.push(
            crate::features::evaluate_policies::dto::EvaluationDiagnostic {
                level: DiagnosticLevel::Info,
                message: format!(
                    "Evaluated {} policies successfully",
                    command.policies.policies().len()
                ),
                policy_id: None,
            },
        );

        Ok(evaluation_decision)
    }

    /// Load schema for evaluation based on the command's evaluation mode
    ///
    /// # Arguments
    ///
    /// * `command` - The evaluation command with schema preferences
    ///
    /// # Returns
    ///
    /// A tuple of (optional schema version, diagnostics)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Strict mode is enabled and schema loading fails
    /// - Schema storage encounters an error in strict mode
    async fn load_schema_for_evaluation(
        &self,
        command: &EvaluatePoliciesCommand<'_>,
    ) -> Result<
        (
            Option<String>,
            Vec<crate::features::evaluate_policies::dto::EvaluationDiagnostic>,
        ),
        EvaluatePoliciesError,
    > {
        let mut diagnostics = vec![];

        match command.evaluation_mode {
            EvaluationMode::NoSchema => {
                debug!("Evaluation mode is NoSchema, skipping schema loading");
                diagnostics.push(
                    crate::features::evaluate_policies::dto::EvaluationDiagnostic {
                        level: DiagnosticLevel::Info,
                        message: "Evaluating without schema (NoSchema mode)".to_string(),
                        policy_id: None,
                    },
                );
                Ok((None, diagnostics))
            }
            EvaluationMode::Strict | EvaluationMode::BestEffortNoSchema => {
                // Try to load the schema
                let schema_load_result = self
                    .schema_storage
                    .load_schema(command.schema_version.clone())
                    .await;

                match schema_load_result {
                    Ok(stored_schema) => {
                        let version = stored_schema.version.clone();
                        info!(
                            version = ?version,
                            "Successfully loaded schema for evaluation"
                        );

                        diagnostics.push(
                            crate::features::evaluate_policies::dto::EvaluationDiagnostic {
                                level: DiagnosticLevel::Info,
                                message: format!(
                                    "Using schema version: {}",
                                    version.as_deref().unwrap_or("latest")
                                ),
                                policy_id: None,
                            },
                        );

                        // TODO: In the future, we should actually use the loaded schema
                        // in the authorization engine. For now, we just track that it was loaded.
                        // The engine would need to be updated to accept a Schema parameter.

                        Ok((version, diagnostics))
                    }
                    Err(e) => {
                        if command.evaluation_mode == EvaluationMode::Strict {
                            warn!("Schema loading failed in Strict mode: {}", e);
                            Err(EvaluatePoliciesError::StrictModeSchemaRequired)
                        } else {
                            // BestEffortNoSchema: log warning and continue without schema
                            warn!(
                                "Schema loading failed in BestEffortNoSchema mode: {}, continuing without schema",
                                e
                            );
                            diagnostics.push(
                                crate::features::evaluate_policies::dto::EvaluationDiagnostic {
                                    level: DiagnosticLevel::Warning,
                                    message: format!(
                                        "Schema loading failed, evaluating without schema: {}",
                                        e
                                    ),
                                    policy_id: None,
                                },
                            );
                            Ok((None, diagnostics))
                        }
                    }
                }
            }
        }
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

        debug!("Cleared policy evaluation cache");

        Ok(())
    }
}

/// Implementation of the EvaluatePoliciesPort trait for EvaluatePoliciesUseCase
///
/// This allows the use case to be used via the port abstraction,
/// enabling dependency inversion for other bounded contexts.
#[async_trait]
impl EvaluatePoliciesPort for EvaluatePoliciesUseCase {
    async fn evaluate(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        self.execute(command).await
    }

    async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError> {
        self.clear_cache().await
    }
}
