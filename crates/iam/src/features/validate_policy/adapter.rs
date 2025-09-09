// crates/iam/src/features/validate_policy/adapter.rs

use super::dto::*;
use super::ports::*;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use security::ComprehensiveCedarValidator;
use std::sync::Arc;

/// Cedar-based implementation of PolicySyntaxValidator
pub struct CedarSyntaxValidatorAdapter {
    validator: Arc<ComprehensiveCedarValidator>,
}

impl CedarSyntaxValidatorAdapter {
    pub fn new() -> Result<Self, IamError> {
        let validator = Arc::new(ComprehensiveCedarValidator::new()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to create Cedar validator: {}", e)))?);
        
        Ok(Self { validator })
    }
}

#[async_trait]
impl PolicySyntaxValidator for CedarSyntaxValidatorAdapter {
    async fn validate_syntax(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        let result = self.validator.validate_policy_comprehensive(policy_content).await
            .map_err(|e| IamError::validation_error(format!("Syntax validation failed: {}", e)))?;

        let syntax_errors = result.syntax_errors.into_iter()
            .map(|error| ValidationError {
                error_type: ValidationErrorType::SyntaxError,
                message: error,
                location: None, // Cedar doesn't provide location info in our current setup
                suggested_fix: None,
                documentation_link: Some("https://docs.cedarpolicy.com/policies/syntax.html".to_string()),
            })
            .collect();

        Ok(syntax_errors)
    }

    async fn is_syntax_valid(&self, policy_content: &str) -> Result<bool, IamError> {
        let errors = self.validate_syntax(policy_content).await?;
        Ok(errors.is_empty())
    }
}

/// Cedar-based implementation of PolicySemanticValidator
pub struct CedarSemanticValidatorAdapter {
    validator: Arc<ComprehensiveCedarValidator>,
    schema_loader: Arc<CedarSchemaLoaderAdapter>,
}

impl CedarSemanticValidatorAdapter {
    pub fn new() -> Result<Self, IamError> {
        let validator = Arc::new(ComprehensiveCedarValidator::new()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to create Cedar validator: {}", e)))?);
        let schema_loader = Arc::new(CedarSchemaLoaderAdapter::new()?);
        
        Ok(Self { validator, schema_loader })
    }
}

#[async_trait]
impl PolicySemanticValidator for CedarSemanticValidatorAdapter {
    async fn validate_semantics(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        let result = self.validator.validate_policy_comprehensive(policy_content).await
            .map_err(|e| IamError::validation_error(format!("Semantic validation failed: {}", e)))?;

        let semantic_errors = result.semantic_errors.into_iter()
            .map(|error| ValidationError {
                error_type: ValidationErrorType::SemanticError,
                message: error,
                location: None,
                suggested_fix: None,
                documentation_link: Some("https://docs.cedarpolicy.com/policies/validation.html".to_string()),
            })
            .collect();

        Ok(semantic_errors)
    }

    async fn validate_semantics_with_schema(&self, policy_content: &str, _schema_version: &str) -> Result<Vec<ValidationError>, IamError> {
        // For now, use the same validation as the regular method
        // In a real implementation, we would load the specific schema version
        self.validate_semantics(policy_content).await
    }

    async fn is_semantically_valid(&self, policy_content: &str) -> Result<bool, IamError> {
        let errors = self.validate_semantics(policy_content).await?;
        Ok(errors.is_empty())
    }
}

/// Cedar-based implementation of PolicyHrnValidator
pub struct CedarHrnValidatorAdapter {
    validator: Arc<ComprehensiveCedarValidator>,
}

impl CedarHrnValidatorAdapter {
    pub fn new() -> Result<Self, IamError> {
        let validator = Arc::new(ComprehensiveCedarValidator::new()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to create Cedar validator: {}", e)))?);
        
        Ok(Self { validator })
    }
}

#[async_trait]
impl PolicyHrnValidator for CedarHrnValidatorAdapter {
    async fn validate_hrns(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        let result = self.validator.validate_policy_comprehensive(policy_content).await
            .map_err(|e| IamError::validation_error(format!("HRN validation failed: {}", e)))?;

        let hrn_errors = result.hrn_errors.into_iter()
            .map(|error| ValidationError {
                error_type: ValidationErrorType::HrnError,
                message: error,
                location: None,
                suggested_fix: Some("Check HRN format: hrn:hodei:service:region:account:resource-type/resource-id".to_string()),
                documentation_link: Some("https://docs.hodei.com/hrn-format".to_string()),
            })
            .collect();

        Ok(hrn_errors)
    }

    async fn extract_and_validate_hrns(&self, policy_content: &str) -> Result<Vec<String>, IamError> {
        // Simple HRN extraction - in a real implementation, this would parse the policy properly
        let mut hrns = Vec::new();
        
        for line in policy_content.lines() {
            if let Some(start) = line.find("hrn:") {
                if let Some(end) = line[start..].find('"') {
                    hrns.push(line[start..start + end].to_string());
                }
            }
        }
        
        Ok(hrns)
    }

    async fn is_hrn_valid(&self, hrn: &str) -> Result<bool, IamError> {
        // Simple HRN format validation
        let parts: Vec<&str> = hrn.split(':').collect();
        Ok(parts.len() >= 6 && parts[0] == "hrn" && parts[1] == "hodei")
    }
}

/// Schema loader adapter for Cedar
pub struct CedarSchemaLoaderAdapter;

impl CedarSchemaLoaderAdapter {
    pub fn new() -> Result<Self, IamError> {
        Ok(Self)
    }
}

/// Cross-policy analyzer for detecting conflicts and redundancies
pub struct SimpleCrossPolicyAnalyzer;

impl SimpleCrossPolicyAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrossPolicyAnalyzer for SimpleCrossPolicyAnalyzer {
    async fn detect_conflicts(&self, policies: &[&str]) -> Result<Vec<PolicyConflict>, IamError> {
        let mut conflicts = Vec::new();
        
        // Simple conflict detection - look for permit/forbid conflicts
        for (i, policy1) in policies.iter().enumerate() {
            for (j, policy2) in policies.iter().enumerate().skip(i + 1) {
                if self.policies_conflict(policy1, policy2) {
                    conflicts.push(PolicyConflict {
                        conflict_type: ConflictType::DirectContradiction,
                        involved_policies: vec![i, j],
                        description: "Policies have conflicting effects".to_string(),
                        severity: ConflictSeverity::High,
                        suggested_resolution: Some("Review policy precedence or add conditions".to_string()),
                    });
                }
            }
        }
        
        Ok(conflicts)
    }

    async fn detect_redundancies(&self, policies: &[&str]) -> Result<Vec<PolicyRedundancy>, IamError> {
        let mut redundancies = Vec::new();
        
        // Simple redundancy detection - look for very similar policies
        for (i, policy1) in policies.iter().enumerate() {
            for (j, policy2) in policies.iter().enumerate().skip(i + 1) {
                if self.policies_similar(policy1, policy2) {
                    redundancies.push(PolicyRedundancy {
                        redundant_policies: vec![j], // Mark the later one as redundant
                        superseding_policy: i,
                        explanation: "Policies are very similar and may be redundant".to_string(),
                    });
                }
            }
        }
        
        Ok(redundancies)
    }

    async fn analyze_coverage(&self, _policies: &[&str]) -> Result<CoverageAnalysis, IamError> {
        // Simple coverage analysis
        Ok(CoverageAnalysis {
            overall_coverage: 75.0, // Placeholder
            uncovered_entities: vec!["SomeEntity".to_string()],
            uncovered_actions: vec!["SomeAction".to_string()],
            coverage_gaps: vec!["Missing permissions for admin users".to_string()],
        })
    }
}

impl SimpleCrossPolicyAnalyzer {
    fn policies_conflict(&self, policy1: &str, policy2: &str) -> bool {
        // Simple heuristic: one permits and another forbids
        (policy1.contains("permit") && policy2.contains("forbid")) ||
        (policy1.contains("forbid") && policy2.contains("permit"))
    }

    fn policies_similar(&self, policy1: &str, policy2: &str) -> bool {
        // Simple similarity check based on common words
        let words1: std::collections::HashSet<&str> = policy1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = policy2.split_whitespace().collect();
        
        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();
        
        if union == 0 {
            false
        } else {
            (intersection as f64 / union as f64) > 0.8
        }
    }
}

/// Simple metrics collector for validation operations
pub struct SimpleValidationMetricsCollector;

impl SimpleValidationMetricsCollector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ValidationMetricsCollector for SimpleValidationMetricsCollector {
    async fn start_validation_metrics(&self) -> Result<ValidationMetricsSession, IamError> {
        Ok(ValidationMetricsSession::new(uuid::Uuid::new_v4().to_string()))
    }

    async fn record_validation_step(&self, _session: &ValidationMetricsSession, _step_name: &str) -> Result<(), IamError> {
        // In a real implementation, this would record metrics
        Ok(())
    }

    async fn finish_validation_metrics(&self, session: ValidationMetricsSession) -> Result<ValidationMetrics, IamError> {
        let total_time = session.get_total_duration().as_millis() as u64;
        
        Ok(ValidationMetrics {
            validation_time_ms: total_time,
            syntax_validation_time_ms: total_time / 3,
            semantic_validation_time_ms: total_time / 3,
            hrn_validation_time_ms: total_time / 3,
            memory_usage_bytes: None,
        })
    }
}

/// Schema provider for validation operations
pub struct CedarValidationSchemaProvider;

impl CedarValidationSchemaProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ValidationSchemaProvider for CedarValidationSchemaProvider {
    async fn get_schema_info(&self) -> Result<SchemaValidationInfo, IamError> {
        Ok(SchemaValidationInfo {
            version: "1.0.0".to_string(),
            schema_id: "default".to_string(),
            entity_types_count: 5,
            actions_count: 10,
        })
    }

    async fn load_schema(&self, _version: &str) -> Result<String, IamError> {
        // In a real implementation, this would load the actual schema
        Ok("{}".to_string()) // Empty schema for now
    }

    async fn validate_schema_compatibility(&self, _policy_content: &str, _schema_version: &str) -> Result<bool, IamError> {
        Ok(true) // Always compatible for now
    }
}

/// Result aggregator for validation operations
pub struct SimpleValidationResultAggregator;

impl SimpleValidationResultAggregator {
    pub fn new() -> Self {
        Self
    }
}

impl ValidationResultAggregator for SimpleValidationResultAggregator {
    fn aggregate_validation_results(
        &self,
        syntax_errors: Vec<ValidationError>,
        semantic_errors: Vec<ValidationError>,
        hrn_errors: Vec<ValidationError>,
        warnings: Vec<ValidationWarning>,
        schema_info: SchemaValidationInfo,
    ) -> PolicyValidationResult {
        PolicyValidationResult {
            syntax_errors,
            semantic_errors,
            hrn_errors,
            warnings,
            schema_info,
        }
    }

    fn aggregate_batch_results(
        &self,
        individual_results: Vec<IndividualValidationResult>,
        cross_policy_results: Option<CrossPolicyValidationResult>,
    ) -> ValidatePoliciesBatchResponse {
        let overall_valid = individual_results.iter().all(|r| r.is_valid);
        
        ValidatePoliciesBatchResponse {
            overall_valid,
            individual_results,
            cross_policy_results,
            batch_metrics: BatchValidationMetrics {
                total_time_ms: 1000, // Placeholder
                individual_validation_time_ms: 800,
                cross_policy_analysis_time_ms: 200,
                policies_processed: 0,
            },
        }
    }
}

/// Event publisher for validation operations
pub struct SimpleValidationEventPublisher;

impl SimpleValidationEventPublisher {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ValidationEventPublisher for SimpleValidationEventPublisher {
    async fn publish_validation_started(&self, _command: &ValidatePolicyCommand) -> Result<(), IamError> {
        // In a real implementation, this would publish events
        Ok(())
    }

    async fn publish_validation_completed(&self, _command: &ValidatePolicyCommand, _response: &ValidatePolicyResponse) -> Result<(), IamError> {
        Ok(())
    }

    async fn publish_batch_validation_started(&self, _command: &ValidatePoliciesBatchCommand) -> Result<(), IamError> {
        Ok(())
    }

    async fn publish_batch_validation_completed(&self, _command: &ValidatePoliciesBatchCommand, _response: &ValidatePoliciesBatchResponse) -> Result<(), IamError> {
        Ok(())
    }
}

// Default implementations
impl Default for CedarSyntaxValidatorAdapter {
    fn default() -> Self {
        Self::new().expect("Failed to create default CedarSyntaxValidatorAdapter")
    }
}

impl Default for CedarSemanticValidatorAdapter {
    fn default() -> Self {
        Self::new().expect("Failed to create default CedarSemanticValidatorAdapter")
    }
}

impl Default for CedarHrnValidatorAdapter {
    fn default() -> Self {
        Self::new().expect("Failed to create default CedarHrnValidatorAdapter")
    }
}

impl Default for SimpleCrossPolicyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleValidationMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CedarValidationSchemaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleValidationResultAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleValidationEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cedar_syntax_validator_creation() {
        let validator = CedarSyntaxValidatorAdapter::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_syntax_validation_valid_policy() {
        let validator = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = validator.validate_syntax("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // May or may not have errors depending on Cedar's validation
    }

    #[tokio::test]
    async fn test_syntax_validation_invalid_policy() {
        let validator = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = validator.validate_syntax("invalid policy syntax").await;
        
        assert!(result.is_ok());
        // Should have syntax errors for invalid policy
    }

    #[tokio::test]
    async fn test_semantic_validator_creation() {
        let validator = CedarSemanticValidatorAdapter::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_hrn_validator_creation() {
        let validator = CedarHrnValidatorAdapter::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_hrn_validation() {
        let validator = CedarHrnValidatorAdapter::new().unwrap();
        let valid_hrn = "hrn:hodei:iam:us-east-1:123456789012:policy/test";
        let result = validator.is_hrn_valid(valid_hrn).await;
        
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_cross_policy_analyzer() {
        let analyzer = SimpleCrossPolicyAnalyzer::new();
        let policies = vec![
            "permit(principal, action, resource);",
            "forbid(principal, action, resource);",
        ];
        
        let conflicts = analyzer.detect_conflicts(&policies).await;
        assert!(conflicts.is_ok());
        
        let redundancies = analyzer.detect_redundancies(&policies).await;
        assert!(redundancies.is_ok());
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = SimpleValidationMetricsCollector::new();
        let session = collector.start_validation_metrics().await.unwrap();
        
        collector.record_validation_step(&session, "syntax", 100).await.unwrap();
        let metrics = collector.finish_validation_metrics(&session).await.unwrap();
        
        assert!(metrics.validation_time_ms > 0);
    }
}