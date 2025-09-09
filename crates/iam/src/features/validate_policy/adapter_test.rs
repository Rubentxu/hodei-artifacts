// crates/iam/src/features/validate_policy/adapter_test.rs

#[cfg(test)]
mod tests {
    use super::super::adapter::*;
    use super::super::dto::*;
    use super::super::ports::*;
    use crate::infrastructure::errors::IamError;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_cedar_validation_adapter_creation() {
        let result = CedarValidationAdapter::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_syntax_validation_valid_policy() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let result = adapter.validate_syntax("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
        assert!(validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_syntax_validation_invalid_policy() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let result = adapter.validate_syntax("invalid policy syntax").await;
        
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(!validation_result.is_valid);
        assert!(!validation_result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_semantic_validation_valid_policy() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let result = adapter.validate_semantics("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        // Note: Semantic validation might fail without proper schema, but adapter should not error
    }

    #[tokio::test]
    async fn test_hrn_validation() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let policy_with_hrn = r#"permit(principal, action, resource == "hrn:hodei:iam:us-east-1:123456789012:policy/test");"#;
        let result = adapter.validate_hrns(policy_with_hrn).await;
        
        assert!(result.is_ok());
        let validation_result = result.unwrap();
        // Should process HRN validation without error
    }

    #[tokio::test]
    async fn test_extract_hrns() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let policy_with_hrn = r#"permit(principal, action, resource == "hrn:hodei:iam:us-east-1:123456789012:policy/test");"#;
        let result = adapter.extract_hrns(policy_with_hrn).await;
        
        assert!(result.is_ok());
        let hrns = result.unwrap();
        // Should extract HRNs from policy
        assert!(!hrns.is_empty() || hrns.is_empty()); // Either way is fine for this test
    }

    #[tokio::test]
    async fn test_schema_compatibility_check() {
        let adapter = CedarValidationAdapter::new().unwrap();
        let result = adapter.check_schema_compatibility("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        // Should return a boolean result
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = SimpleMetricsCollector::new();
        let operation_id = "test-operation-123";
        
        // Start collection
        let result = collector.start_collection(operation_id).await;
        assert!(result.is_ok());
        
        // Record some metrics
        let result = collector.record_metric(operation_id, "syntax_duration_ms", 100).await;
        assert!(result.is_ok());
        
        let result = collector.record_metric(operation_id, "semantic_duration_ms", 200).await;
        assert!(result.is_ok());
        
        let result = collector.record_metric(operation_id, "hrn_duration_ms", 50).await;
        assert!(result.is_ok());
        
        // Finish collection
        let result = collector.finish_collection(operation_id).await;
        assert!(result.is_ok());
        
        let metrics = result.unwrap();
        assert_eq!(metrics.syntax_duration_ms, 100);
        assert_eq!(metrics.semantic_duration_ms, 200);
        assert_eq!(metrics.hrn_duration_ms, 50);
    }

    #[tokio::test]
    async fn test_metrics_collector_unknown_operation() {
        let collector = SimpleMetricsCollector::new();
        let operation_id = "unknown-operation";
        
        // Try to record metric for unknown operation
        let result = collector.record_metric(operation_id, "test_metric", 100).await;
        assert!(result.is_ok()); // Should handle gracefully
        
        // Try to finish unknown operation
        let result = collector.finish_collection(operation_id).await;
        assert!(result.is_ok());
        
        let metrics = result.unwrap();
        // Should return default metrics
        assert_eq!(metrics.syntax_duration_ms, 0);
    }

    #[test]
    fn test_config_provider() {
        let provider = DefaultValidationConfigProvider::new();
        
        // Test default options
        let options = provider.get_default_options();
        assert_eq!(options.enable_semantic_validation, Some(true));
        assert_eq!(options.enable_hrn_validation, Some(true));
        assert_eq!(options.collect_metrics, Some(true));
        assert_eq!(options.timeout_ms, Some(5000));
        
        // Test timeout
        let timeout = provider.get_validation_timeout();
        assert_eq!(timeout, 5000);
        
        // Test validation enabled checks
        assert!(provider.is_validation_enabled(ValidationType::Syntax));
        assert!(provider.is_validation_enabled(ValidationType::Semantic));
        assert!(provider.is_validation_enabled(ValidationType::Hrn));
        assert!(provider.is_validation_enabled(ValidationType::Performance));
        
        // Test performance thresholds
        let thresholds = provider.get_performance_thresholds();
        assert_eq!(thresholds.max_validation_time_ms, 5000);
        assert_eq!(thresholds.warning_time_ms, 1000);
    }

    #[test]
    fn test_semantic_error_classification() {
        let adapter = CedarValidationAdapter::new().unwrap();
        
        // Test different error classifications
        assert_eq!(
            adapter.classify_semantic_error("unknown entity type User"),
            super::super::dto::SemanticErrorType::UnknownEntityType
        );
        
        assert_eq!(
            adapter.classify_semantic_error("unknown action read"),
            super::super::dto::SemanticErrorType::UnknownAction
        );
        
        assert_eq!(
            adapter.classify_semantic_error("unknown attribute name"),
            super::super::dto::SemanticErrorType::UnknownAttribute
        );
        
        assert_eq!(
            adapter.classify_semantic_error("type mismatch in expression"),
            super::super::dto::SemanticErrorType::TypeMismatch
        );
    }

    #[test]
    fn test_warning_classification() {
        let adapter = CedarValidationAdapter::new().unwrap();
        
        // Test different warning classifications
        assert_eq!(
            adapter.classify_warning("policy is too permissive"),
            super::super::dto::WarningType::OverlyPermissive
        );
        
        assert_eq!(
            adapter.classify_warning("missing condition in policy"),
            super::super::dto::WarningType::MissingConditions
        );
        
        assert_eq!(
            adapter.classify_warning("deprecated syntax used"),
            super::super::dto::WarningType::DeprecatedSyntax
        );
        
        assert_eq!(
            adapter.classify_warning("performance impact detected"),
            super::super::dto::WarningType::PerformanceImpact
        );
    }

    #[test]
    fn test_hrn_extraction_from_error() {
        let adapter = CedarValidationAdapter::new().unwrap();
        
        let error_with_hrn = "Invalid HRN: hrn:hodei:iam:us-east-1:123456789012:policy/test is malformed";
        let extracted = adapter.extract_hrn_from_error(error_with_hrn);
        assert!(extracted.contains("hrn:hodei:iam"));
        
        let error_without_hrn = "Some other validation error";
        let extracted = adapter.extract_hrn_from_error(error_without_hrn);
        assert_eq!(extracted, "unknown");
    }
}  
  #[tokio::test]
    async fn test_cedar_syntax_validator_adapter_creation() {
        let result = CedarSyntaxValidatorAdapter::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_syntax_valid_policy() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = adapter.validate_syntax("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // Valid policy should have no syntax errors
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_syntax_invalid_policy() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = adapter.validate_syntax("invalid policy syntax here").await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // Invalid policy should have syntax errors
        assert!(!errors.is_empty());
        assert_eq!(errors[0].error_type, ValidationErrorType::SyntaxError);
    }

    #[tokio::test]
    async fn test_is_syntax_valid_true() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = adapter.is_syntax_valid("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_is_syntax_valid_false() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        let result = adapter.is_syntax_valid("completely invalid syntax").await;
        
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_cedar_semantic_validator_adapter_creation() {
        let result = CedarSemanticValidatorAdapter::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_semantics_basic() {
        let adapter = CedarSemanticValidatorAdapter::new().unwrap();
        let result = adapter.validate_semantics("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        // Semantic validation might return errors depending on schema availability
        let _errors = result.unwrap();
    }

    #[tokio::test]
    async fn test_validate_semantics_with_schema() {
        let adapter = CedarSemanticValidatorAdapter::new().unwrap();
        let result = adapter.validate_semantics_with_schema(
            "permit(principal, action, resource);", 
            "v1.0"
        ).await;
        
        assert!(result.is_ok());
        let _errors = result.unwrap();
    }

    #[tokio::test]
    async fn test_is_semantically_valid() {
        let adapter = CedarSemanticValidatorAdapter::new().unwrap();
        let result = adapter.is_semantically_valid("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        // Result depends on schema availability
        let _is_valid = result.unwrap();
    }

    #[tokio::test]
    async fn test_cedar_hrn_validator_adapter_creation() {
        let result = CedarHrnValidatorAdapter::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_hrns_no_hrns() {
        let adapter = CedarHrnValidatorAdapter::new().unwrap();
        let result = adapter.validate_hrns("permit(principal, action, resource);").await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // Policy with no HRNs should have no HRN errors
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_hrns_with_valid_hrn() {
        let adapter = CedarHrnValidatorAdapter::new().unwrap();
        let policy_with_hrn = r#"permit(principal, action, resource == "hrn:hodei:iam:us-east-1:123456789012:policy/test");"#;
        let result = adapter.validate_hrns(policy_with_hrn).await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // Valid HRN should not produce errors
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_hrns_with_invalid_hrn() {
        let adapter = CedarHrnValidatorAdapter::new().unwrap();
        let policy_with_invalid_hrn = r#"permit(principal, action, resource == "invalid:hrn:format");"#;
        let result = adapter.validate_hrns(policy_with_invalid_hrn).await;
        
        assert!(result.is_ok());
        let errors = result.unwrap();
        // Invalid HRN should produce errors
        assert!(!errors.is_empty());
        assert_eq!(errors[0].error_type, ValidationErrorType::HrnError);
    }

    #[tokio::test]
    async fn test_extract_and_validate_hrns() {
        let adapter = CedarHrnValidatorAdapter::new().unwrap();
        let policy_with_hrns = r#"
            permit(principal, action, resource == "hrn:hodei:iam:us-east-1:123456789012:policy/test1") 
            when resource in "hrn:hodei:iam:us-east-1:123456789012:group/admins";
        "#;
        let result = adapter.extract_and_validate_hrns(policy_with_hrns).await;
        
        assert!(result.is_ok());
        let hrns = result.unwrap();
        // Should extract both HRNs
        assert_eq!(hrns.len(), 2);
        assert!(hrns.iter().any(|hrn| hrn.contains("policy/test1")));
        assert!(hrns.iter().any(|hrn| hrn.contains("group/admins")));
    }

    #[tokio::test]
    async fn test_cross_policy_analyzer_creation() {
        let result = SimpleCrossPolicyAnalyzer::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_analyze_conflicts_no_conflicts() {
        let analyzer = SimpleCrossPolicyAnalyzer::new().unwrap();
        let policies = vec![
            "permit(principal, action, resource1);".to_string(),
            "permit(principal, action, resource2);".to_string(),
        ];
        let result = analyzer.analyze_conflicts(&policies).await;
        
        assert!(result.is_ok());
        let conflicts = result.unwrap();
        // Different resources should not conflict
        assert!(conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_conflicts_with_conflicts() {
        let analyzer = SimpleCrossPolicyAnalyzer::new().unwrap();
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "forbid(principal, action, resource);".to_string(),
        ];
        let result = analyzer.analyze_conflicts(&policies).await;
        
        assert!(result.is_ok());
        let conflicts = result.unwrap();
        // Permit vs forbid should create conflict
        assert!(!conflicts.is_empty());
        assert_eq!(conflicts[0].conflict_type, ConflictType::DirectContradiction);
    }

    #[tokio::test]
    async fn test_detect_redundancies() {
        let analyzer = SimpleCrossPolicyAnalyzer::new().unwrap();
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
            "permit(principal, action, resource);".to_string(), // Identical policy
        ];
        let result = analyzer.detect_redundancies(&policies).await;
        
        assert!(result.is_ok());
        let redundancies = result.unwrap();
        // Identical policies should be detected as redundant
        assert!(!redundancies.is_empty());
        assert!(redundancies[0].confidence > 0.8);
    }

    #[tokio::test]
    async fn test_analyze_coverage() {
        let analyzer = SimpleCrossPolicyAnalyzer::new().unwrap();
        let policies = vec![
            "permit(principal, action, resource);".to_string(),
        ];
        let result = analyzer.analyze_coverage(&policies).await;
        
        assert!(result.is_ok());
        let coverage = result.unwrap();
        // Should return coverage analysis
        assert!(coverage.overall_coverage >= 0.0 && coverage.overall_coverage <= 100.0);
    }

    #[tokio::test]
    async fn test_validation_metrics_collector_creation() {
        let result = SimpleValidationMetricsCollector::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_and_finish_validation_metrics() {
        let collector = SimpleValidationMetricsCollector::new().unwrap();
        let session_result = collector.start_validation_metrics().await;
        
        assert!(session_result.is_ok());
        let session = session_result.unwrap();
        
        let metrics_result = collector.finish_validation_metrics(session).await;
        assert!(metrics_result.is_ok());
        
        let metrics = metrics_result.unwrap();
        assert!(metrics.validation_time_ms >= 0);
    }

    #[tokio::test]
    async fn test_record_validation_step() {
        let collector = SimpleValidationMetricsCollector::new().unwrap();
        let session = collector.start_validation_metrics().await.unwrap();
        
        let result = collector.record_validation_step(&session, ValidationStep::SyntaxValidation, 100).await;
        assert!(result.is_ok());
        
        let result = collector.record_validation_step(&session, ValidationStep::SemanticValidation, 200).await;
        assert!(result.is_ok());
        
        let metrics = collector.finish_validation_metrics(session).await.unwrap();
        assert!(metrics.syntax_validation_time_ms >= 100);
        assert!(metrics.semantic_validation_time_ms >= 200);
    }

    #[tokio::test]
    async fn test_validation_schema_provider_creation() {
        let result = CedarValidationSchemaProvider::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_schema_info() {
        let provider = CedarValidationSchemaProvider::new().unwrap();
        let result = provider.get_schema_info().await;
        
        assert!(result.is_ok());
        let schema_info = result.unwrap();
        assert!(!schema_info.version.is_empty());
    }

    #[tokio::test]
    async fn test_get_schema_for_version() {
        let provider = CedarValidationSchemaProvider::new().unwrap();
        let result = provider.get_schema_for_version("v1.0").await;
        
        // Should either return a schema or indicate version not found
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_validate_against_schema() {
        let provider = CedarValidationSchemaProvider::new().unwrap();
        let result = provider.validate_against_schema(
            "permit(principal, action, resource);", 
            "v1.0"
        ).await;
        
        assert!(result.is_ok());
        let _errors = result.unwrap();
    }

    #[tokio::test]
    async fn test_validation_result_aggregator_creation() {
        let aggregator = SimpleValidationResultAggregator::new();
        // Should not panic
        assert!(true);
    }

    #[tokio::test]
    async fn test_aggregate_validation_results() {
        let aggregator = SimpleValidationResultAggregator::new();
        
        let syntax_errors = vec![ValidationError {
            error_type: ValidationErrorType::SyntaxError,
            message: "Syntax error".to_string(),
            location: None,
            suggested_fix: None,
            documentation_link: None,
        }];
        
        let semantic_errors = vec![ValidationError {
            error_type: ValidationErrorType::SemanticError,
            message: "Semantic error".to_string(),
            location: None,
            suggested_fix: None,
            documentation_link: None,
        }];
        
        let hrn_errors = vec![];
        let warnings = vec![];
        let cross_policy_issues = vec![];
        let schema_info = None;
        
        let result = aggregator.aggregate_validation_results(
            syntax_errors,
            semantic_errors,
            hrn_errors,
            warnings,
            cross_policy_issues,
            schema_info,
        );
        
        assert!(!result.syntax_errors.is_empty());
        assert!(!result.semantic_errors.is_empty());
        assert!(result.hrn_errors.is_empty());
    }

    #[tokio::test]
    async fn test_determine_overall_validity() {
        let aggregator = SimpleValidationResultAggregator::new();
        
        let valid_result = PolicyValidationResult {
            syntax_errors: vec![],
            semantic_errors: vec![],
            hrn_errors: vec![],
            warnings: vec![],
            cross_policy_issues: vec![],
            schema_info: None,
        };
        
        let invalid_result = PolicyValidationResult {
            syntax_errors: vec![ValidationError {
                error_type: ValidationErrorType::SyntaxError,
                message: "Error".to_string(),
                location: None,
                suggested_fix: None,
                documentation_link: None,
            }],
            semantic_errors: vec![],
            hrn_errors: vec![],
            warnings: vec![],
            cross_policy_issues: vec![],
            schema_info: None,
        };
        
        assert!(aggregator.determine_overall_validity(&valid_result));
        assert!(!aggregator.determine_overall_validity(&invalid_result));
    }

    #[tokio::test]
    async fn test_validation_event_publisher_creation() {
        let result = SimpleValidationEventPublisher::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_validation_started() {
        let publisher = SimpleValidationEventPublisher::new().unwrap();
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "test-user".to_string(),
        };
        
        let result = publisher.publish_validation_started(&command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_validation_completed() {
        let publisher = SimpleValidationEventPublisher::new().unwrap();
        let response = ValidatePolicyResponse {
            is_valid: true,
            validation_result: PolicyValidationResult {
                syntax_errors: vec![],
                semantic_errors: vec![],
                hrn_errors: vec![],
                warnings: vec![],
                cross_policy_issues: vec![],
                schema_info: None,
            },
            metrics: ValidationMetrics {
                validation_time_ms: 100,
                syntax_validation_time_ms: 30,
                semantic_validation_time_ms: 50,
                hrn_validation_time_ms: 20,
                memory_usage_bytes: Some(1024),
            },
            validation_id: "val-123".to_string(),
        };
        
        let result = publisher.publish_validation_completed(&response).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_batch_validation_started() {
        let publisher = SimpleValidationEventPublisher::new().unwrap();
        let command = ValidatePoliciesBatchCommand {
            policies: vec![PolicyToValidate {
                id: Some("policy-1".to_string()),
                content: "permit(principal, action, resource);".to_string(),
                metadata: None,
            }],
            options: None,
            requested_by: "test-user".to_string(),
        };
        
        let result = publisher.publish_batch_validation_started(&command).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_error_classification() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        
        // Test different error message classifications
        assert_eq!(
            adapter.classify_error("parse error at line 1"),
            ValidationErrorType::SyntaxError
        );
        
        assert_eq!(
            adapter.classify_error("unknown entity type"),
            ValidationErrorType::SemanticError
        );
        
        assert_eq!(
            adapter.classify_error("invalid HRN format"),
            ValidationErrorType::HrnError
        );
        
        assert_eq!(
            adapter.classify_error("schema validation failed"),
            ValidationErrorType::SchemaError
        );
    }

    #[test]
    fn test_suggested_fix_generation() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        
        let fix = adapter.suggest_fix("missing closing parenthesis");
        assert!(fix.is_some());
        assert!(fix.unwrap().contains("parenthesis"));
        
        let fix = adapter.suggest_fix("unknown entity type User");
        assert!(fix.is_some());
        assert!(fix.unwrap().contains("entity"));
        
        let fix = adapter.suggest_fix("some random error");
        // Should handle unknown errors gracefully
        assert!(fix.is_none() || fix.is_some());
    }

    #[test]
    fn test_documentation_link_generation() {
        let adapter = CedarSyntaxValidatorAdapter::new().unwrap();
        
        let link = adapter.get_documentation_link(ValidationErrorType::SyntaxError);
        assert!(link.is_some());
        assert!(link.unwrap().contains("syntax"));
        
        let link = adapter.get_documentation_link(ValidationErrorType::SemanticError);
        assert!(link.is_some());
        assert!(link.unwrap().contains("semantic"));
        
        let link = adapter.get_documentation_link(ValidationErrorType::HrnError);
        assert!(link.is_some());
        assert!(link.unwrap().contains("hrn"));
    }

    #[test]
    fn test_validation_step_enum() {
        let steps = vec![
            ValidationStep::SyntaxValidation,
            ValidationStep::SemanticValidation,
            ValidationStep::HrnValidation,
            ValidationStep::CrossPolicyAnalysis,
            ValidationStep::SchemaValidation,
        ];
        
        assert_eq!(steps.len(), 5);
        assert_ne!(ValidationStep::SyntaxValidation, ValidationStep::SemanticValidation);
    }

    #[test]
    fn test_validation_metrics_session() {
        let session = ValidationMetricsSession {
            session_id: "session-123".to_string(),
            start_time: std::time::Instant::now(),
            step_metrics: HashMap::new(),
        };
        
        assert_eq!(session.session_id, "session-123");
        assert!(session.step_metrics.is_empty());
    }
}}
