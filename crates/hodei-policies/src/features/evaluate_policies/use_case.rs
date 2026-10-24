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
    /// 2. Build the immutable snapshot (policies + entities) of this request
    /// 3. Build the authorization request
    /// 4. Evaluate the request against that snapshot
    /// 5. Return the decision
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

        // Step 2: Build the immutable snapshot for this request
        //
        // The snapshot holds the policies and the entities of this request
        // only. Nothing is stored in the engine between requests, so a
        // concurrent evaluation cannot observe or contaminate this one.
        let policy_documents: Vec<crate::internal::engine::types::PolicyDocument> = command
            .policies
            .policies()
            .iter()
            .map(|policy| {
                crate::internal::engine::types::PolicyDocument::new(
                    policy.id().to_string(),
                    policy.content().to_string(),
                )
            })
            .collect();

        let snapshot = self
            .engine
            .build_snapshot(&policy_documents, command.entities)
            .map_err(|e| EvaluatePoliciesError::PolicyLoadError(e.to_string()))?;

        info!(
            policies = snapshot.policy_count(),
            entities = snapshot.entity_count(),
            "Built the evaluation snapshot"
        );

        // Step 3: Build engine request
        let engine_request = crate::internal::engine::types::EngineRequest::new(
            command.request.principal,
            command.request.action,
            command.request.resource,
        )
        .with_context(command.request.context.clone().unwrap_or_default());

        // Step 4: Evaluate authorization against this request's snapshot
        let decision = self
            .engine
            .evaluate(&snapshot, &engine_request)
            .map_err(|e| EvaluatePoliciesError::EvaluationError(e.to_string()))?;

        debug!(
            decision = decision.is_allowed(),
            "Policy evaluation completed"
        );

        // Step 5: Map engine decision to use case decision
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

        // Step 6: Build and return evaluation decision
        let mut evaluation_decision = EvaluationDecision {
            decision: mapped_decision,
            determining_policies: decision.determining_policies().to_vec(),
            reasons: vec![decision.reason().to_string()],
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

                        // The loaded schema is not applied to the request yet: the
                        // engine evaluates in schema-less mode. Applying it (and
                        // making the schema the authority of the action namespace)
                        // is scheduled for H1b; see
                        // `ADR/011-h1a-identity-and-decision-core.md` and BASELINE
                        // defect O8.

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
    /// The engine keeps no cache: policies and entities live in the
    /// per-request snapshot and are dropped with it. This method therefore
    /// does nothing; it is kept so the port contract stays stable.
    pub async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError> {
        debug!("The evaluation engine holds no cache; nothing to clear");

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
