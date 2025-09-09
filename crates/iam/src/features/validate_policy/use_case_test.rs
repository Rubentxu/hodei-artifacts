// crates/iam/src/features/validate_policy/use_case_test.rs

use crate::features::validate_policy::dto::*;
use crate::features::validate_policy::ports::*;
use crate::features::validate_policy::use_case::ValidatePolicyUseCase;
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use std::sync::Arc;

// Mock implementations for testing
struct MockPolicySyntaxValidator {
    should_fail: bool,
}

#[async_trait]
impl PolicySyntaxValidator for MockPolicySyntaxValidator {
    async fn validate_syntax(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        if self.should_fail {
            Ok(vec![ValidationError {
                error_type: ValidationErrorType::SyntaxError,
                message: "Mock syntax error".to_string(),
                location: None,
                suggested_fix: None,
                documentation_link: None,
            }])
        } else {
            Ok(Vec::new())
        }
    }

    async fn is_syntax_valid(&self, policy_content: &str) -> Result<bool, IamError> {
        let errors = self.validate_syntax(policy_content).await?;
        Ok(errors.is_empty())
    }
}

struct MockPolicySemanticValidator {
    should_fail: bool,
}

#[async_trait]
impl PolicySemanticValidator for MockPolicySemanticValidator {
    async fn validate_semantics(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        if self.should_fail {
            Ok(vec![ValidationError {
                error_type: ValidationErrorType::SemanticError,
                message: "Mock semantic error".to_string(),
                location: None,
                suggested_fix: None,
                documentation_link: None,
            }])
        } else {
            Ok(Vec::new())
        }
    }

    async fn validate_semantics_with_schema(&self, policy_content: &str, _schema_version: &str) -> Result<Vec<ValidationError>, IamError> {
        self.validate_semantics(policy_content).await
    }

    async fn is_semantically_valid(&self, policy_content: &str) -> Result<bool, IamError> {
        let errors = self.validate_semantics(policy_content).await?;
        Ok(errors.is_empty())
    }
}

struct MockPolicyHrnValidator {
    should_fail: bool,
}

#[async_trait]
impl PolicyHrnValidator for MockPolicyHrnValidator {
    async fn validate_hrns(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
        if self.should_fail {
            Ok(vec![ValidationError {
                error_type: ValidationErrorType::HrnError,
                message: "Mock HRN error".to_string(),
                location: None,
                suggested_fix: None,
                documentation_link: None,
            }])
        } else {
            Ok(Vec::new())
        }
    }

    async fn extract_and_validate_hrns(&self, _policy_content: &str) -> Result<Vec<String>, IamError> {
        Ok(Vec::new())
    }
}

struct MockCrossPolicyAnalyzer;

#[async_trait]
impl CrossPolicyAnalyzer for MockCrossPolicyAnalyzer {
    async fn detect_conflicts(&self, _policies: &[&str]) -> Result<Vec<PolicyConflict>, IamError> {
        Ok(Vec::new())
    }

    async fn find_redundancies(&self, _policies: &[&str]) -> Result<Vec<PolicyRedundancy>, IamError> {
        Ok(Vec::new())
    }

    async fn analyze_coverage(&self, _policies: &[&str]) -> Result<CoverageAnalysis, IamError> {
        Ok(CoverageAnalysis {
            overall_coverage: 80.0,
            entity_coverage: 75.0,
            action_coverage: 85.0,
            uncovered_entities: Vec::new(),
            uncovered_actions: Vec::new(),
        })
    }
}

struct MockValidationMetricsCollector;

#[async_trait]
impl ValidationMetricsCollector for MockValidationMetricsCollector {
    async fn start_validation_metrics(&self) -> Result<ValidationMetricsSession, IamError> {
        Ok(ValidationMetricsSession::new("test-session".to_string()))
    }

    async fn record_validation_step(&self, _session: &ValidationMetricsSession, _step_name: &str) -> Result<(), IamError> {
        Ok(())
    }

    async fn finish_validation_metrics(&self, _session: ValidationMetricsSession) -> Result<ValidationMetrics, IamError> {
        Ok(ValidationMetrics {
            validation_time_ms: 100,
            memory_usage_bytes: 1024,
            validation_steps: 3,
            schema_load_time_ms: 10,
        })
    }
}

struct MockValidationSchemaProvider;

#[async_trait]
impl ValidationSchemaProvider for MockValidationSchemaProvider {
    async fn get_schema_info(&self) -> Result<SchemaValidationInfo, IamError> {
        Ok(SchemaValidationInfo {
            version: "1.0.0".to_string(),
            schema_id: "test-schema".to_string(),
            entity_types_count: 5,
            actions_count: 10,
        })
    }

    async fn get_schema_info_for_version(&self, _version: &str) -> Result<SchemaValidationInfo, IamError> {
        self.get_schema_info().await
    }

    async fn is_schema_version_supported(&self, _version: &str) -> Result<bool, IamError> {
        Ok(true)
    }
}

struct MockValidationResultAggregator;

impl ValidationResultAggregator for MockValidationResultAggregator {
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
        batch_metrics: BatchValidationMetrics,
    ) -> ValidatePoliciesBatchResponse {
        ValidatePoliciesBatchResponse::new(individual_results, cross_policy_results, batch_metrics)
    }
}

struct MockValidationEventPublisher;

#[async_trait]
impl ValidationEventPublisher for MockValidationEventPublisher {
    async fn publish_validation_started(&self, _command: &ValidatePolicyCommand) -> Result<(), IamError> {
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

fn create_use_case(
    syntax_should_fail: bool,
    semantic_should_fail: bool,
    hrn_should_fail: bool,
) -> ValidatePolicyUseCase {
    ValidatePolicyUseCase::new(
        Arc::new(MockPolicySyntaxValidator { should_fail: syntax_should_fail }),
        Arc::new(MockPolicySemanticValidator { should_fail: semantic_should_fail }),
        Arc::new(MockPolicyHrnValidator { should_fail: hrn_should_fail }),
        Arc::new(MockCrossPolicyAnalyzer),
        Arc::new(MockValidationMetricsCollector),
        Arc::new(MockValidationSchemaProvider),
        Arc::new(MockValidationResultAggregator),
        Arc::new(MockValidationEventPublisher),
    )
}

#[tokio::test]
async fn test_validate_policy_success() {
    let use_case = create_use_case(false, false, false);
    
    let command = ValidatePolicyCommand::new(
        "permit(principal, action, resource);".to_string(),
        "test_user".to_string(),
    );

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.is_valid);
    assert!(response.validation_result.syntax_errors.is_empty());
    assert!(response.validation_result.semantic_errors.is_empty());
    assert!(response.validation_result.hrn_errors.is_empty());
}

#[tokio::test]
async fn test_validate_policy_syntax_error() {
    let use_case = create_use_case(true, false, false);
    
    let command = ValidatePolicyCommand::new(
        "invalid syntax".to_string(),
        "test_user".to_string(),
    );

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(!response.is_valid);
    assert!(!response.validation_result.syntax_errors.is_empty());
    assert_eq!(response.validation_result.syntax_errors[0].message, "Mock syntax error");
}

#[tokio::test]
async fn test_validate_policy_semantic_error() {
    let use_case = create_use_case(false, true, false);
    
    let command = ValidatePolicyCommand::new(
        "permit(principal == UnknownEntity::\"test\", action, resource);".to_string(),
        "test_user".to_string(),
    );

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(!response.is_valid);
    assert!(!response.validation_result.semantic_errors.is_empty());
    assert_eq!(response.validation_result.semantic_errors[0].message, "Mock semantic error");
}

#[tokio::test]
async fn test_validate_policy_hrn_error() {
    let use_case = create_use_case(false, false, true);
    
    let command = ValidatePolicyCommand::new(
        "permit(principal == User::\"invalid-hrn\", action, resource);".to_string(),
        "test_user".to_string(),
    );

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(!response.is_valid);
    assert!(!response.validation_result.hrn_errors.is_empty());
    assert_eq!(response.validation_result.hrn_errors[0].message, "Mock HRN error");
}

#[tokio::test]
async fn test_validate_policy_invalid_command() {
    let use_case = create_use_case(false, false, false);
    
    let command = ValidatePolicyCommand::new(
        "".to_string(), // Empty content
        "test_user".to_string(),
    );

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        IamError::InvalidInput(msg) => {
            assert!(msg.contains("cannot be empty"));
        }
        _ => panic!("Expected InvalidInput error"),
    }
}

#[tokio::test]
async fn test_validate_policies_batch_success() {
    let use_case = create_use_case(false, false, false);
    
    let policies = vec![
        PolicyToValidate::new("permit(principal, action, resource);".to_string()),
        PolicyToValidate::new("forbid(principal, action, resource);".to_string()),
    ];
    
    let command = ValidatePoliciesBatchCommand::new(policies, "test_user".to_string());

    let result = use_case.execute_batch(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.overall_valid);
    assert_eq!(response.individual_results.len(), 2);
    assert_eq!(response.batch_metrics.policies_processed, 2);
    assert_eq!(response.batch_metrics.policies_passed, 2);
}

#[tokio::test]
async fn test_validate_policies_batch_with_errors() {
    let use_case = create_use_case(true, false, false); // Syntax errors
    
    let policies = vec![
        PolicyToValidate::new("invalid syntax 1".to_string()),
        PolicyToValidate::new("invalid syntax 2".to_string()),
    ];
    
    let command = ValidatePoliciesBatchCommand::new(policies, "test_user".to_string());

    let result = use_case.execute_batch(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(!response.overall_valid);
    assert_eq!(response.individual_results.len(), 2);
    assert!(response.individual_results.iter().all(|r| !r.is_valid));
    assert_eq!(response.batch_metrics.policies_processed, 2);
    assert_eq!(response.batch_metrics.policies_passed, 0);
}

#[tokio::test]
async fn test_validate_policies_batch_empty() {
    let use_case = create_use_case(false, false, false);
    
    let command = ValidatePoliciesBatchCommand::new(Vec::new(), "test_user".to_string());

    let result = use_case.execute_batch(command).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        IamError::InvalidInput(msg) => {
            assert!(msg.contains("At least one policy"));
        }
        _ => panic!("Expected InvalidInput error"),
    }
}

#[tokio::test]
async fn test_validation_with_options() {
    let use_case = create_use_case(false, false, false);
    
    let options = ValidationOptions {
        include_warnings: Some(false),
        deep_validation: Some(true),
        schema_version: Some("1.0.0".to_string()),
        timeout_ms: Some(1000),
    };
    
    let command = ValidatePolicyCommand::new(
        "permit(principal, action, resource);".to_string(),
        "test_user".to_string(),
    ).with_options(options);

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.is_valid);
    assert_eq!(response.validation_result.schema_info.version, "1.0.0");
}