// crates/iam/src/features/validate_policy/ports.rs

use crate::features::validate_policy::dto::*;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;

/// Port for comprehensive policy validation operations
/// This is the main validation service for the validate_policy feature
#[async_trait]
pub trait PolicyValidationService: Send + Sync {
    /// Validate a single policy comprehensively
    async fn validate_policy(&self, command: ValidatePolicyCommand) -> Result<ValidatePolicyResponse, IamError>;
    
    /// Validate multiple policies in batch
    async fn validate_policies_batch(&self, command: ValidatePoliciesBatchCommand) -> Result<ValidatePoliciesBatchResponse, IamError>;
}

/// Port for syntax validation operations
/// Segregated interface for syntax-only validation
#[async_trait]
pub trait PolicySyntaxValidator: Send + Sync {
    /// Validate policy syntax only
    async fn validate_syntax(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError>;
    
    /// Check if policy content has valid Cedar syntax
    async fn is_syntax_valid(&self, policy_content: &str) -> Result<bool, IamError>;
}

/// Port for semantic validation operations
/// Segregated interface for semantic validation against schema
#[async_trait]
pub trait PolicySemanticValidator: Send + Sync {
    /// Validate policy semantics against schema
    async fn validate_semantics(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError>;
    
    /// Validate policy semantics with specific schema version
    async fn validate_semantics_with_schema(&self, policy_content: &str, schema_version: &str) -> Result<Vec<ValidationError>, IamError>;
    
    /// Check if policy is semantically valid
    async fn is_semantically_valid(&self, policy_content: &str) -> Result<bool, IamError>;
}

/// Port for HRN validation operations
/// Segregated interface for Hodei Resource Name validation
#[async_trait]
pub trait PolicyHrnValidator: Send + Sync {
    /// Validate HRNs referenced in the policy
    async fn validate_hrns(&self, policy_content: &str) -> Result<Vec<ValidationError>, IamError>;
    
    /// Extract and validate all HRNs from policy
    async fn extract_and_validate_hrns(&self, policy_content: &str) -> Result<Vec<String>, IamError>;
}

/// Port for cross-policy analysis operations
/// Segregated interface for analyzing relationships between policies
#[async_trait]
pub trait CrossPolicyAnalyzer: Send + Sync {
    /// Detect conflicts between policies
    async fn detect_conflicts(&self, policies: &[&str]) -> Result<Vec<PolicyConflict>, IamError>;
    
    /// Find redundant policies
    async fn find_redundancies(&self, policies: &[&str]) -> Result<Vec<PolicyRedundancy>, IamError>;
    
    /// Analyze coverage of policies against schema
    async fn analyze_coverage(&self, policies: &[&str]) -> Result<CoverageAnalysis, IamError>;
}

/// Port for validation metrics collection
/// Segregated interface for performance monitoring
#[async_trait]
pub trait ValidationMetricsCollector: Send + Sync {
    /// Start metrics collection for a validation operation
    async fn start_validation_metrics(&self) -> Result<ValidationMetricsSession, IamError>;
    
    /// Record validation step
    async fn record_validation_step(&self, session: &ValidationMetricsSession, step_name: &str) -> Result<(), IamError>;
    
    /// Finish metrics collection and return results
    async fn finish_validation_metrics(&self, session: ValidationMetricsSession) -> Result<ValidationMetrics, IamError>;
}

/// Port for schema information retrieval
/// Segregated interface for schema-related operations specific to validation
#[async_trait]
pub trait ValidationSchemaProvider: Send + Sync {
    /// Get schema information for validation
    async fn get_schema_info(&self) -> Result<SchemaValidationInfo, IamError>;
    
    /// Get schema information for specific version
    async fn get_schema_info_for_version(&self, version: &str) -> Result<SchemaValidationInfo, IamError>;
    
    /// Check if schema version is supported
    async fn is_schema_version_supported(&self, version: &str) -> Result<bool, IamError>;
}

/// Port for validation result aggregation
/// Segregated interface for combining validation results
pub trait ValidationResultAggregator: Send + Sync {
    /// Aggregate individual validation results into a comprehensive result
    fn aggregate_validation_results(
        &self,
        syntax_errors: Vec<ValidationError>,
        semantic_errors: Vec<ValidationError>,
        hrn_errors: Vec<ValidationError>,
        warnings: Vec<ValidationWarning>,
        schema_info: SchemaValidationInfo,
    ) -> PolicyValidationResult;
    
    /// Aggregate batch validation results
    fn aggregate_batch_results(
        &self,
        individual_results: Vec<IndividualValidationResult>,
        cross_policy_results: Option<CrossPolicyValidationResult>,
        batch_metrics: BatchValidationMetrics,
    ) -> ValidatePoliciesBatchResponse;
}

/// Port for validation event publishing
/// Segregated interface for audit and monitoring events
#[async_trait]
pub trait ValidationEventPublisher: Send + Sync {
    /// Publish policy validation started event
    async fn publish_validation_started(&self, command: &ValidatePolicyCommand) -> Result<(), IamError>;
    
    /// Publish policy validation completed event
    async fn publish_validation_completed(&self, command: &ValidatePolicyCommand, response: &ValidatePolicyResponse) -> Result<(), IamError>;
    
    /// Publish batch validation started event
    async fn publish_batch_validation_started(&self, command: &ValidatePoliciesBatchCommand) -> Result<(), IamError>;
    
    /// Publish batch validation completed event
    async fn publish_batch_validation_completed(&self, command: &ValidatePoliciesBatchCommand, response: &ValidatePoliciesBatchResponse) -> Result<(), IamError>;
}

/// Metrics session for tracking validation performance
#[derive(Debug, Clone)]
pub struct ValidationMetricsSession {
    pub session_id: String,
    pub started_at: std::time::Instant,
    pub steps: Vec<ValidationStep>,
}

/// Individual validation step for metrics
#[derive(Debug, Clone)]
pub struct ValidationStep {
    pub name: String,
    pub started_at: std::time::Instant,
    pub completed_at: Option<std::time::Instant>,
}

impl ValidationMetricsSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            started_at: std::time::Instant::now(),
            steps: Vec::new(),
        }
    }
    
    pub fn add_step(&mut self, step_name: String) {
        self.steps.push(ValidationStep {
            name: step_name,
            started_at: std::time::Instant::now(),
            completed_at: None,
        });
    }
    
    pub fn complete_last_step(&mut self) {
        if let Some(last_step) = self.steps.last_mut() {
            last_step.completed_at = Some(std::time::Instant::now());
        }
    }
    
    pub fn get_total_duration(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

/// Port for cross-policy analysis operations
/// Segregated interface for analyzing relationships between policies
#[async_trait]
pub trait CrossPolicyAnalyzer: Send + Sync {
    /// Detect conflicts between policies
    async fn detect_conflicts(&self, policies: &[&str]) -> Result<Vec<PolicyConflict>, IamError>;
    
    /// Find redundant policies
    async fn detect_redundancies(&self, policies: &[&str]) -> Result<Vec<PolicyRedundancy>, IamError>;
    
    /// Analyze coverage of policies against schema
    async fn analyze_coverage(&self, policies: &[&str]) -> Result<CoverageAnalysis, IamError>;
}

/// Port for validation metrics collection
/// Segregated interface for performance monitoring
#[async_trait]
pub trait ValidationMetricsCollector: Send + Sync {
    /// Start metrics collection for a validation operation
    async fn start_validation_metrics(&self) -> Result<ValidationMetricsSession, IamError>;
    
    /// Record validation step
    async fn record_validation_step(&self, session: &ValidationMetricsSession, step_name: &str) -> Result<(), IamError>;
    
    /// Finish metrics collection and return results
    async fn finish_validation_metrics(&self, session: ValidationMetricsSession) -> Result<ValidationMetrics, IamError>;
}

/// Port for schema information retrieval
/// Segregated interface for schema-related operations specific to validation
#[async_trait]
pub trait ValidationSchemaProvider: Send + Sync {
    /// Get schema information for validation
    async fn get_schema_info(&self) -> Result<SchemaValidationInfo, IamError>;
    
    /// Load schema content for specific version
    async fn load_schema(&self, version: &str) -> Result<String, IamError>;
    
    /// Check schema compatibility with policy
    async fn validate_schema_compatibility(&self, policy_content: &str, schema_version: &str) -> Result<bool, IamError>;
}

/// Port for validation result aggregation
/// Segregated interface for combining validation results
pub trait ValidationResultAggregator: Send + Sync {
    /// Aggregate individual validation results into a comprehensive result
    fn aggregate_validation_results(
        &self,
        syntax_errors: Vec<ValidationError>,
        semantic_errors: Vec<ValidationError>,
        hrn_errors: Vec<ValidationError>,
        warnings: Vec<ValidationWarning>,
        schema_info: SchemaValidationInfo,
    ) -> PolicyValidationResult;
    
    /// Aggregate batch validation results
    fn aggregate_batch_results(
        &self,
        individual_results: Vec<IndividualValidationResult>,
        cross_policy_results: Option<CrossPolicyValidationResult>,
    ) -> ValidatePoliciesBatchResponse;
}

/// Port for validation event publishing
/// Segregated interface for audit and monitoring events
#[async_trait]
pub trait ValidationEventPublisher: Send + Sync {
    /// Publish policy validation started event
    async fn publish_validation_started(&self, command: &ValidatePolicyCommand) -> Result<(), IamError>;
    
    /// Publish policy validation completed event
    async fn publish_validation_completed(&self, command: &ValidatePolicyCommand, response: &ValidatePolicyResponse) -> Result<(), IamError>;
    
    /// Publish batch validation started event
    async fn publish_batch_validation_started(&self, command: &ValidatePoliciesBatchCommand) -> Result<(), IamError>;
    
    /// Publish batch validation completed event
    async fn publish_batch_validation_completed(&self, command: &ValidatePoliciesBatchCommand, response: &ValidatePoliciesBatchResponse) -> Result<(), IamError>;
}

/// Metrics session for tracking validation performance
#[derive(Debug, Clone)]
pub struct ValidationMetricsSession {
    pub session_id: String,
    pub start_time: std::time::Instant,
}

/// Individual validation step for metrics
#[derive(Debug, Clone)]
pub struct ValidationStep {
    pub name: String,
    pub started_at: std::time::Instant,
    pub completed_at: Option<std::time::Instant>,
}

impl ValidationMetricsSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            start_time: std::time::Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_metrics_session_creation() {
        let session = ValidationMetricsSession::new("test-session".to_string());
        assert_eq!(session.session_id, "test-session");
    }

    #[test]
    fn test_validation_step_creation() {
        let step = ValidationStep {
            name: "syntax_validation".to_string(),
            started_at: std::time::Instant::now(),
            completed_at: None,
        };
        
        assert_eq!(step.name, "syntax_validation");
        assert!(step.completed_at.is_none());
    }
}