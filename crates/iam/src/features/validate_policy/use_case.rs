// crates/iam/src/features/validate_policy/use_case.rs

use crate::features::validate_policy::dto::*;
use crate::features::validate_policy::ports::*;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

/// Use case for comprehensive policy validation
/// Orchestrates all validation operations using segregated ports
pub struct ValidatePolicyUseCase {
    syntax_validator: Arc<dyn PolicySyntaxValidator>,
    semantic_validator: Arc<dyn PolicySemanticValidator>,
    hrn_validator: Arc<dyn PolicyHrnValidator>,
    cross_policy_analyzer: Arc<dyn CrossPolicyAnalyzer>,
    metrics_collector: Arc<dyn ValidationMetricsCollector>,
    schema_provider: Arc<dyn ValidationSchemaProvider>,
    result_aggregator: Arc<dyn ValidationResultAggregator>,
    event_publisher: Arc<dyn ValidationEventPublisher>,
}

impl ValidatePolicyUseCase {
    /// Create a new validate policy use case with all required dependencies
    pub fn new(
        syntax_validator: Arc<dyn PolicySyntaxValidator>,
        semantic_validator: Arc<dyn PolicySemanticValidator>,
        hrn_validator: Arc<dyn PolicyHrnValidator>,
        cross_policy_analyzer: Arc<dyn CrossPolicyAnalyzer>,
        metrics_collector: Arc<dyn ValidationMetricsCollector>,
        schema_provider: Arc<dyn ValidationSchemaProvider>,
        result_aggregator: Arc<dyn ValidationResultAggregator>,
        event_publisher: Arc<dyn ValidationEventPublisher>,
    ) -> Self {
        Self {
            syntax_validator,
            semantic_validator,
            hrn_validator,
            cross_policy_analyzer,
            metrics_collector,
            schema_provider,
            result_aggregator,
            event_publisher,
        }
    }

    /// Execute single policy validation
    pub async fn execute(&self, command: ValidatePolicyCommand) -> Result<ValidatePolicyResponse, IamError> {
        // 1. Validate command
        command.validate()?;

        // 2. Publish validation started event
        self.event_publisher.publish_validation_started(&command).await?;

        // 3. Start metrics collection
        let metrics_session = self.metrics_collector.start_validation_metrics().await?;

        // 4. Perform comprehensive validation
        let validation_result = self.perform_comprehensive_validation(&command, &metrics_session).await?;

        // 5. Finish metrics collection
        let metrics = self.metrics_collector.finish_validation_metrics(metrics_session).await?;

        // 6. Create response
        let response = ValidatePolicyResponse::new(validation_result, metrics);

        // 7. Publish validation completed event
        self.event_publisher.publish_validation_completed(&command, &response).await?;

        Ok(response)
    }

    /// Execute batch policy validation
    pub async fn execute_batch(&self, command: ValidatePoliciesBatchCommand) -> Result<ValidatePoliciesBatchResponse, IamError> {
        // 1. Validate command
        command.validate()?;

        // 2. Publish batch validation started event
        self.event_publisher.publish_batch_validation_started(&command).await?;

        let batch_start_time = Instant::now();

        // 3. Validate individual policies
        let mut individual_results = Vec::new();
        let mut policies_content = Vec::new();

        for (index, policy) in command.policies.iter().enumerate() {
            // Create individual validation command
            let individual_command = ValidatePolicyCommand {
                content: policy.content.clone(),
                options: command.options.clone(),
                requested_by: command.requested_by.clone(),
            };

            // Validate individual policy
            let individual_response = self.execute(individual_command).await?;
            
            // Store policy content for cross-policy analysis
            policies_content.push(policy.content.as_str());

            // Create individual result
            let individual_result = IndividualValidationResult {
                index,
                policy_id: policy.id.clone(),
                is_valid: individual_response.is_valid,
                validation_result: individual_response.validation_result,
            };

            individual_results.push(individual_result);
        }

        // 4. Perform cross-policy analysis if enabled
        let cross_policy_results = if command.options.as_ref().and_then(|o| o.deep_validation).unwrap_or(true) {
            Some(self.perform_cross_policy_analysis(&policies_content).await?)
        } else {
            None
        };

        // 5. Calculate batch metrics
        let batch_metrics = self.calculate_batch_metrics(&individual_results, batch_start_time);

        // 6. Aggregate results
        let response = self.result_aggregator.aggregate_batch_results(
            individual_results,
            cross_policy_results,
            batch_metrics,
        );

        // 7. Publish batch validation completed event
        self.event_publisher.publish_batch_validation_completed(&command, &response).await?;

        Ok(response)
    }

    /// Perform comprehensive validation for a single policy
    async fn perform_comprehensive_validation(
        &self,
        command: &ValidatePolicyCommand,
        metrics_session: &ValidationMetricsSession,
    ) -> Result<PolicyValidationResult, IamError> {
        let mut warnings = Vec::new();

        // 1. Syntax validation
        self.metrics_collector.record_validation_step(metrics_session, "syntax_validation").await?;
        let syntax_errors = self.syntax_validator.validate_syntax(&command.content).await?;

        // 2. Semantic validation (only if syntax is valid)
        self.metrics_collector.record_validation_step(metrics_session, "semantic_validation").await?;
        let semantic_errors = if syntax_errors.is_empty() {
            if let Some(schema_version) = command.options.as_ref().and_then(|o| o.schema_version.as_ref()) {
                self.semantic_validator.validate_semantics_with_schema(&command.content, schema_version).await?
            } else {
                self.semantic_validator.validate_semantics(&command.content).await?
            }
        } else {
            // Skip semantic validation if syntax is invalid
            warnings.push(ValidationWarning {
                message: "Semantic validation skipped due to syntax errors".to_string(),
                location: None,
                severity: WarningSeverity::Medium,
            });
            Vec::new()
        };

        // 3. HRN validation
        self.metrics_collector.record_validation_step(metrics_session, "hrn_validation").await?;
        let hrn_errors = self.hrn_validator.validate_hrns(&command.content).await?;

        // 4. Get schema information
        self.metrics_collector.record_validation_step(metrics_session, "schema_info_retrieval").await?;
        let schema_info = if let Some(schema_version) = command.options.as_ref().and_then(|o| o.schema_version.as_ref()) {
            self.schema_provider.get_schema_info_for_version(schema_version).await?
        } else {
            self.schema_provider.get_schema_info().await?
        };

        // 5. Add warnings if requested
        if !command.options.as_ref().and_then(|o| o.include_warnings).unwrap_or(true) {
            warnings.clear();
        }

        // 6. Aggregate results
        let validation_result = self.result_aggregator.aggregate_validation_results(
            syntax_errors,
            semantic_errors,
            hrn_errors,
            warnings,
            schema_info,
        );

        Ok(validation_result)
    }

    /// Perform cross-policy analysis
    async fn perform_cross_policy_analysis(&self, policies: &[&str]) -> Result<CrossPolicyValidationResult, IamError> {
        // 1. Detect conflicts
        let conflicts = self.cross_policy_analyzer.detect_conflicts(policies).await?;

        // 2. Find redundancies
        let redundancies = self.cross_policy_analyzer.find_redundancies(policies).await?;

        // 3. Analyze coverage
        let coverage_analysis = Some(self.cross_policy_analyzer.analyze_coverage(policies).await?);

        Ok(CrossPolicyValidationResult {
            conflicts,
            redundancies,
            coverage_analysis,
        })
    }

    /// Calculate batch validation metrics
    fn calculate_batch_metrics(&self, individual_results: &[IndividualValidationResult], start_time: Instant) -> BatchValidationMetrics {
        let total_time_ms = start_time.elapsed().as_millis() as u64;
        let policies_processed = individual_results.len();
        let policies_passed = individual_results.iter().filter(|r| r.is_valid).count();
        let average_time_per_policy_ms = if policies_processed > 0 {
            total_time_ms / policies_processed as u64
        } else {
            0
        };

        // Estimate memory usage (this would be more accurate with actual memory tracking)
        let total_memory_usage_bytes = policies_processed * 1024; // Rough estimate

        BatchValidationMetrics {
            total_time_ms,
            average_time_per_policy_ms,
            policies_processed,
            policies_passed,
            total_memory_usage_bytes,
        }
    }
}

#[async_trait]
impl PolicyValidationService for ValidatePolicyUseCase {
    async fn validate_policy(&self, command: ValidatePolicyCommand) -> Result<ValidatePolicyResponse, IamError> {
        self.execute(command).await
    }

    async fn validate_policies_batch(&self, command: ValidatePoliciesBatchCommand) -> Result<ValidatePoliciesBatchResponse, IamError> {
        self.execute_batch(command).await
    }
}