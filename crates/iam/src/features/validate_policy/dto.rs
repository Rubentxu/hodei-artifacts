// crates/iam/src/features/validate_policy/dto.rs

use crate::infrastructure::errors::IamError;
use serde::{Deserialize, Serialize};

/// Command for validating a single policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatePolicyCommand {
    /// The Cedar policy content to validate
    pub content: String,
    /// Optional validation options
    pub options: Option<ValidationOptions>,
    /// User requesting the validation (for audit)
    pub requested_by: String,
}

/// Command for validating multiple policies in batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatePoliciesBatchCommand {
    /// List of policies to validate
    pub policies: Vec<PolicyToValidate>,
    /// Optional validation options
    pub options: Option<ValidationOptions>,
    /// User requesting the validation (for audit)
    pub requested_by: String,
}

/// Individual policy in a batch validation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyToValidate {
    /// Optional identifier for the policy
    pub id: Option<String>,
    /// The Cedar policy content
    pub content: String,
    /// Optional metadata for this specific policy
    pub metadata: Option<PolicyValidationMetadata>,
}

/// Validation options for policy validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationOptions {
    /// Whether to include warnings in the response
    pub include_warnings: Option<bool>,
    /// Whether to perform deep semantic validation
    pub deep_validation: Option<bool>,
    /// Whether to validate against specific schema version
    pub schema_version: Option<String>,
    /// Maximum validation time in milliseconds
    pub timeout_ms: Option<u64>,
}

/// Metadata for policy validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyValidationMetadata {
    /// Name of the policy being validated
    pub name: Option<String>,
    /// Description of the policy
    pub description: Option<String>,
    /// Tags associated with the policy
    pub tags: Option<Vec<String>>,
}

/// Response for single policy validation
#[derive(Debug, Clone, Serialize)]
pub struct ValidatePolicyResponse {
    /// Whether the policy is valid
    pub is_valid: bool,
    /// Validation result details
    pub validation_result: PolicyValidationResult,
    /// Performance metrics
    pub metrics: ValidationMetrics,
}

/// Response for batch policy validation
#[derive(Debug, Clone, Serialize)]
pub struct ValidatePoliciesBatchResponse {
    /// Overall batch validation status
    pub overall_valid: bool,
    /// Individual validation results
    pub individual_results: Vec<IndividualValidationResult>,
    /// Cross-policy validation results (conflicts, etc.)
    pub cross_policy_results: Option<CrossPolicyValidationResult>,
    /// Batch processing metrics
    pub batch_metrics: BatchValidationMetrics,
}

/// Individual policy validation result in a batch
#[derive(Debug, Clone, Serialize)]
pub struct IndividualValidationResult {
    /// Index of the policy in the batch
    pub index: usize,
    /// Optional policy ID if provided
    pub policy_id: Option<String>,
    /// Whether this individual policy is valid
    pub is_valid: bool,
    /// Validation details
    pub validation_result: PolicyValidationResult,
}

/// Detailed policy validation result
#[derive(Debug, Clone, Serialize)]
pub struct PolicyValidationResult {
    /// Syntax validation errors
    pub syntax_errors: Vec<ValidationError>,
    /// Semantic validation errors
    pub semantic_errors: Vec<ValidationError>,
    /// HRN validation errors
    pub hrn_errors: Vec<ValidationError>,
    /// Validation warnings
    pub warnings: Vec<ValidationWarning>,
    /// Schema information used for validation
    pub schema_info: SchemaValidationInfo,
}

/// Cross-policy validation results
#[derive(Debug, Clone, Serialize)]
pub struct CrossPolicyValidationResult {
    /// Detected policy conflicts
    pub conflicts: Vec<PolicyConflict>,
    /// Redundant policies detected
    pub redundancies: Vec<PolicyRedundancy>,
    /// Coverage analysis
    pub coverage_analysis: Option<CoverageAnalysis>,
}

/// Validation error details
#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    /// Type of validation error
    pub error_type: ValidationErrorType,
    /// Human-readable error message
    pub message: String,
    /// Location in the policy where the error occurred
    pub location: Option<PolicyLocation>,
    /// Suggested fix for the error
    pub suggested_fix: Option<String>,
    /// Reference to relevant documentation
    pub documentation_link: Option<String>,
}

/// Validation warning details
#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    /// Warning message
    pub message: String,
    /// Location in the policy where the warning applies
    pub location: Option<PolicyLocation>,
    /// Severity level of the warning
    pub severity: WarningSeverity,
}

/// Types of validation errors
#[derive(Debug, Clone, Serialize)]
pub enum ValidationErrorType {
    SyntaxError,
    SemanticError,
    HrnError,
    SchemaViolation,
    TypeMismatch,
    UnknownEntity,
    UnknownAction,
    UnknownAttribute,
    ConstraintViolation,
}

/// Warning severity levels
#[derive(Debug, Clone, Serialize)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

/// Location in a policy
#[derive(Debug, Clone, Serialize)]
pub struct PolicyLocation {
    /// Line number (1-based)
    pub line: u32,
    /// Column number (1-based)
    pub column: u32,
}

/// Schema information used for validation
#[derive(Debug, Clone, Serialize)]
pub struct SchemaValidationInfo {
    /// Schema version used
    pub version: String,
    /// Schema identifier
    pub schema_id: String,
    /// Number of entity types in schema
    pub entity_types_count: usize,
    /// Number of actions in schema
    pub actions_count: usize,
}

/// Policy conflict information
#[derive(Debug, Clone, Serialize)]
pub struct PolicyConflict {
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Policies involved in the conflict
    pub involved_policies: Vec<usize>,
    /// Description of the conflict
    pub description: String,
    /// Suggested resolution
    pub suggested_resolution: Option<String>,
}

/// Policy redundancy information
#[derive(Debug, Clone, Serialize)]
pub struct PolicyRedundancy {
    /// Redundant policies
    pub redundant_policies: Vec<usize>,
    /// Description of the redundancy
    pub description: String,
    /// Suggested action
    pub suggested_action: Option<String>,
}

/// Coverage analysis information
#[derive(Debug, Clone, Serialize)]
pub struct CoverageAnalysis {
    /// Overall coverage percentage
    pub overall_coverage: f64,
    /// Entity types coverage
    pub entity_coverage: f64,
    /// Actions coverage
    pub action_coverage: f64,
    /// Uncovered entities
    pub uncovered_entities: Vec<String>,
    /// Uncovered actions
    pub uncovered_actions: Vec<String>,
}

/// Types of policy conflicts
#[derive(Debug, Clone, Serialize)]
pub enum ConflictType {
    PermitDenyConflict,
    OverlappingConditions,
    InconsistentPermissions,
    CircularDependency,
}

/// Validation performance metrics
#[derive(Debug, Clone, Serialize)]
pub struct ValidationMetrics {
    /// Total validation time in milliseconds
    pub validation_time_ms: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: usize,
    /// Number of validation steps performed
    pub validation_steps: u32,
    /// Schema loading time in milliseconds
    pub schema_load_time_ms: u64,
}

/// Batch validation performance metrics
#[derive(Debug, Clone, Serialize)]
pub struct BatchValidationMetrics {
    /// Total batch processing time
    pub total_time_ms: u64,
    /// Average time per policy
    pub average_time_per_policy_ms: u64,
    /// Number of policies processed
    pub policies_processed: usize,
    /// Number of policies that passed validation
    pub policies_passed: usize,
    /// Memory usage for the entire batch
    pub total_memory_usage_bytes: usize,
}

impl ValidatePolicyCommand {
    pub fn new(content: String, requested_by: String) -> Self {
        Self {
            content,
            options: None,
            requested_by,
        }
    }

    pub fn with_options(mut self, options: ValidationOptions) -> Self {
        self.options = Some(options);
        self
    }

    pub fn validate(&self) -> Result<(), IamError> {
        if self.content.trim().is_empty() {
            return Err(IamError::InvalidInput("Policy content cannot be empty".to_string()));
        }

        if self.requested_by.trim().is_empty() {
            return Err(IamError::InvalidInput("Requested by cannot be empty".to_string()));
        }

        Ok(())
    }
}

impl ValidatePoliciesBatchCommand {
    pub fn new(policies: Vec<PolicyToValidate>, requested_by: String) -> Self {
        Self {
            policies,
            options: None,
            requested_by,
        }
    }

    pub fn with_options(mut self, options: ValidationOptions) -> Self {
        self.options = Some(options);
        self
    }

    pub fn validate(&self) -> Result<(), IamError> {
        if self.policies.is_empty() {
            return Err(IamError::InvalidInput("At least one policy must be provided".to_string()));
        }

        if self.requested_by.trim().is_empty() {
            return Err(IamError::InvalidInput("Requested by cannot be empty".to_string()));
        }

        for (index, policy) in self.policies.iter().enumerate() {
            if policy.content.trim().is_empty() {
                return Err(IamError::InvalidInput(format!("Policy at index {} has empty content", index)));
            }
        }

        Ok(())
    }
}

impl PolicyToValidate {
    pub fn new(content: String) -> Self {
        Self {
            id: None,
            content,
            metadata: None,
        }
    }

    pub fn with_id(mut self, id: String) -> Self {
        self.id = Some(id);
        self
    }

    pub fn with_metadata(mut self, metadata: PolicyValidationMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: None,
            timeout_ms: Some(5000), // 5 seconds default timeout
        }
    }
}

impl ValidatePolicyResponse {
    pub fn new(validation_result: PolicyValidationResult, metrics: ValidationMetrics) -> Self {
        let is_valid = validation_result.syntax_errors.is_empty() 
            && validation_result.semantic_errors.is_empty() 
            && validation_result.hrn_errors.is_empty();

        Self {
            is_valid,
            validation_result,
            metrics,
        }
    }
}

impl ValidatePoliciesBatchResponse {
    pub fn new(
        individual_results: Vec<IndividualValidationResult>,
        cross_policy_results: Option<CrossPolicyValidationResult>,
        batch_metrics: BatchValidationMetrics,
    ) -> Self {
        let overall_valid = individual_results.iter().all(|r| r.is_valid)
            && cross_policy_results.as_ref().map_or(true, |cpr| cpr.conflicts.is_empty());

        Self {
            overall_valid,
            individual_results,
            cross_policy_results,
            batch_metrics,
        }
    }
}