//! Use case for evaluating policies in the playground
//!
//! This use case orchestrates the ad-hoc evaluation of Cedar policies against
//! authorization requests in a playground environment, without requiring
//! persistence of policies or schemas.

use super::dto::{EvaluationDiagnostics, PlaygroundEvaluateCommand, PlaygroundEvaluateResult};
use super::error::PlaygroundEvaluateError;
use super::ports::{
    ContextConverterPort, PlaygroundEvaluatePort, PolicyEvaluatorPort, PolicyValidatorPort,
    SchemaLoaderPort,
};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

/// Use case for playground policy evaluation
///
/// This use case provides ad-hoc policy evaluation capabilities for testing
/// and experimentation. It supports:
/// - Inline Cedar schemas or references to stored schemas
/// - Inline Cedar policies (not persisted)
/// - Authorization requests with custom context
/// - Detailed diagnostics and error reporting
///
/// # Architecture
///
/// This use case depends on several ports for its functionality:
/// - `SchemaLoaderPort`: Loads schemas (inline or from storage)
/// - `PolicyValidatorPort`: Validates policies against schemas
/// - `PolicyEvaluatorPort`: Evaluates authorization requests
/// - `ContextConverterPort`: Converts context attributes to Cedar format
///
/// All dependencies are injected via trait objects, enabling full testability
/// and compliance with the Dependency Inversion Principle.
pub struct PlaygroundEvaluateUseCase {
    /// Schema loader for inline or stored schemas
    schema_loader: Arc<dyn SchemaLoaderPort>,

    /// Policy validator for schema-based validation
    policy_validator: Arc<dyn PolicyValidatorPort>,

    /// Policy evaluator for authorization decisions
    policy_evaluator: Arc<dyn PolicyEvaluatorPort>,

    /// Context converter for attribute translation
    context_converter: Arc<dyn ContextConverterPort>,
}

impl PlaygroundEvaluateUseCase {
    /// Create a new playground evaluate use case
    ///
    /// # Arguments
    ///
    /// * `schema_loader` - Port for loading schemas
    /// * `policy_validator` - Port for validating policies
    /// * `policy_evaluator` - Port for evaluating requests
    /// * `context_converter` - Port for converting context attributes
    pub fn new(
        schema_loader: Arc<dyn SchemaLoaderPort>,
        policy_validator: Arc<dyn PolicyValidatorPort>,
        policy_evaluator: Arc<dyn PolicyEvaluatorPort>,
        context_converter: Arc<dyn ContextConverterPort>,
    ) -> Self {
        Self {
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        }
    }

    /// Execute the playground evaluation
    ///
    /// This method performs the complete evaluation workflow:
    /// 1. Validates the command
    /// 2. Loads the schema (inline or from storage)
    /// 3. Validates policies against the schema
    /// 4. Converts the authorization request
    /// 5. Evaluates policies and returns decision with diagnostics
    ///
    /// # Arguments
    ///
    /// * `command` - The playground evaluation command
    ///
    /// # Returns
    ///
    /// An evaluation result containing the decision, determining policies,
    /// and detailed diagnostics
    ///
    /// # Errors
    ///
    /// Returns an error if any step of the evaluation fails:
    /// - Command validation failure
    /// - Schema loading/parsing failure
    /// - Policy validation failure
    /// - Request evaluation failure
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = PlaygroundEvaluateCommand::new_with_inline_schema(
    ///     schema_json,
    ///     vec![policy_text],
    ///     request,
    /// );
    /// let result = use_case.execute(command).await?;
    /// println!("Decision: {}", result.decision);
    /// ```
    #[instrument(skip(self, command), fields(
        has_inline_schema = command.inline_schema.is_some(),
        schema_version = ?command.schema_version,
        policy_count = command.inline_policies.len()
    ))]
    pub async fn execute(
        &self,
        command: PlaygroundEvaluateCommand,
    ) -> Result<PlaygroundEvaluateResult, PlaygroundEvaluateError> {
        info!("Starting playground policy evaluation");

        // Step 1: Validate command
        command.validate().map_err(|e| {
            warn!("Command validation failed: {}", e);
            PlaygroundEvaluateError::InvalidCommand(e)
        })?;

        debug!("Command validated successfully");

        // Step 2: Load schema
        let schema = self
            .schema_loader
            .load_schema(
                command.inline_schema.clone(),
                command.schema_version.clone(),
            )
            .await
            .map_err(|e| {
                warn!("Schema loading failed: {}", e);
                e
            })?;

        info!("Schema loaded successfully");

        // Step 3: Validate policies against schema
        let validation_errors = self
            .policy_validator
            .validate_policies(&command.inline_policies, &schema)
            .await
            .map_err(|e| {
                warn!("Policy validation failed: {}", e);
                e
            })?;

        // Initialize diagnostics
        let mut diagnostics = EvaluationDiagnostics::new(
            command.inline_policies.len(),
            0, // Will be updated after evaluation
        );
        diagnostics = diagnostics.with_schema_validation();

        // Add validation errors to diagnostics
        if !validation_errors.is_empty() {
            warn!("Found {} validation errors", validation_errors.len());
            for error in &validation_errors {
                diagnostics.add_validation_error(error.clone());
            }
        } else {
            info!("All policies validated successfully");
        }

        // Step 4: Convert context attributes
        let _context = self
            .context_converter
            .convert_context(&command.request.context)
            .map_err(|e| {
                warn!("Context conversion failed: {}", e);
                e
            })?;

        debug!("Context attributes converted");

        // Step 5: Evaluate policies
        let (decision, determining_policies) = self
            .policy_evaluator
            .evaluate(&command.request, &command.inline_policies, &schema)
            .await
            .map_err(|e| {
                warn!("Policy evaluation failed: {}", e);
                e
            })?;

        // Update diagnostics with matched policies count
        diagnostics.matched_policies = determining_policies.len();

        info!(
            decision = %decision,
            determining_policies = determining_policies.len(),
            "Playground evaluation completed successfully"
        );

        // Step 6: Build and return result
        let result = PlaygroundEvaluateResult::new(decision, determining_policies, diagnostics);

        // Add validation errors as result errors if any
        if !validation_errors.is_empty() {
            Ok(result.with_errors(validation_errors))
        } else {
            Ok(result)
        }
    }
}

/// Implementation of PlaygroundEvaluatePort trait for PlaygroundEvaluateUseCase
#[async_trait]
impl PlaygroundEvaluatePort for PlaygroundEvaluateUseCase {
    async fn evaluate(
        &self,
        command: PlaygroundEvaluateCommand,
    ) -> Result<PlaygroundEvaluateResult, PlaygroundEvaluateError> {
        self.execute(command).await
    }
}
