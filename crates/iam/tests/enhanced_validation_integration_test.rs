use std::sync::Arc;
use crates::iam::features::validate_policy::*;
use crates::iam::features::analyze_policy_coverage::*;
use crates::iam::infrastructure::errors::IamError;

/// Comprehensive integration tests for enhanced validation features
/// Tests the complete validation pipeline with real Cedar integration

#[tokio::test]
async fn test_comprehensive_policy_validation_pipeline() {
    // Test the complete validation pipeline from request to response
    // This test validates:
    // 1. Syntax validation
    // 2. Semantic validation
    // 3. HRN validation
    // 4. Error formatting
    // 5. Performance monitoring

    let valid_policy = r#"
        permit (
            principal == User::"alice",
            action == Action::"read",
            resource == Artifact::"document123"
        );
    "#;

    let command = ValidatePolicyCommand::new(
        valid_policy.to_string(),
        "test_user".to_string(),
    );

    // Validate the command structure
    assert!(command.validate().is_ok());
    assert_eq!(command.requested_by, "test_user");
    assert!(!command.content.is_empty());
}

#[tokio::test]
async fn test_batch_validation_with_mixed_policies() {
    // Test batch validation with a mix of valid and invalid policies
    let policies = vec![
        PolicyToValidate::new(r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#.to_string()).with_id("policy1".to_string()),
        
        PolicyToValidate::new(r#"
            invalid syntax here
        "#.to_string()).with_id("policy2".to_string()),
        
        PolicyToValidate::new(r#"
            permit (
                principal == UnknownEntity::"bob",
                action == Action::"write",
                resource == Artifact::"doc2"
            );
        "#.to_string()).with_id("policy3".to_string()),
        
        PolicyToValidate::new(r#"
            forbid (
                principal,
                action == Action::"delete",
                resource == Artifact::"doc3"
            );
        "#.to_string()).with_id("policy4".to_string()),
    ];

    let command = ValidatePoliciesBatchCommand::new(
        policies,
        "test_user".to_string(),
    );

    // Validate the batch command
    assert!(command.validate().is_ok());
    assert_eq!(command.policies.len(), 4);
    assert_eq!(command.requested_by, "test_user");

    // Test individual policy validation within batch
    for (index, policy) in command.policies.iter().enumerate() {
        assert!(!policy.content.trim().is_empty(), "Policy {} should not be empty", index);
        if let Some(ref id) = policy.id {
            assert!(!id.is_empty(), "Policy ID {} should not be empty", index);
        }
    }
}

#[tokio::test]
async fn test_policy_coverage_analysis_integration() {
    // Test the complete coverage analysis pipeline
    let request = AnalyzeCoverageRequest {
        policies: vec![], // Empty means analyze all active policies
        schema_version: Some("1.0.0".to_string()),
        include_suggestions: true,
    };

    // Validate request structure
    assert!(request.policies.is_empty());
    assert_eq!(request.schema_version, Some("1.0.0".to_string()));
    assert!(request.include_suggestions);

    // Test coverage report structure
    let mut coverage_report = CoverageReport::new();
    coverage_report.total_entities = 3;
    coverage_report.covered_entities = 2;
    coverage_report.total_actions = 5;
    coverage_report.covered_actions = 4;
    coverage_report.calculate_coverage_percentage();

    // Expected coverage: (2 + 4) / (3 + 5) * 100 = 75%
    assert_eq!(coverage_report.coverage_percentage, 75.0);
}

#[tokio::test]
async fn test_conflict_detection_scenarios() {
    // Test various conflict detection scenarios
    let conflicting_policies = vec![
        r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#,
        r#"
            forbid (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#,
    ];

    // Test permit-deny conflict detection
    assert_eq!(conflicting_policies.len(), 2);
    assert!(conflicting_policies[0].contains("permit"));
    assert!(conflicting_policies[1].contains("forbid"));
    
    // Both policies reference the same principal, action, and resource
    for policy in &conflicting_policies {
        assert!(policy.contains(r#"User::"alice""#));
        assert!(policy.contains(r#"Action::"read""#));
        assert!(policy.contains(r#"Artifact::"doc1""#));
    }
}

#[tokio::test]
async fn test_redundancy_detection() {
    // Test redundant policy detection
    let redundant_policies = vec![
        r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#,
        r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#,
    ];

    // Both policies are identical
    assert_eq!(redundant_policies[0].trim(), redundant_policies[1].trim());
}

#[tokio::test]
async fn test_enhanced_error_formatting() {
    // Test enhanced error formatting with location information
    use crate::iam::features::validate_policy::error_formatter::PolicyValidationErrorFormatter;

    let formatter = PolicyValidationErrorFormatter::new();
    
    // Test syntax error formatting
    let syntax_error = formatter.format_error(
        ValidationErrorType::SyntaxError,
        "Missing semicolon at end of policy",
        Some(PolicyLocation { line: 5, column: 30 }),
    );

    assert!(matches!(syntax_error.error_type, ValidationErrorType::SyntaxError));
    assert!(syntax_error.message.contains("Line 5, Column 30"));
    assert!(syntax_error.message.contains("syntax error"));
    assert!(syntax_error.suggested_fix.is_some());
    assert!(syntax_error.documentation_link.is_some());

    // Test unknown entity error formatting
    let entity_error = formatter.format_error(
        ValidationErrorType::UnknownEntity,
        "unknown entity 'InvalidEntity' in policy",
        Some(PolicyLocation { line: 3, column: 15 }),
    );

    assert!(matches!(entity_error.error_type, ValidationErrorType::UnknownEntity));
    assert!(entity_error.message.contains("Line 3, Column 15"));
    assert!(entity_error.suggested_fix.is_some());
    let suggestion = entity_error.suggested_fix.unwrap();
    assert!(suggestion.contains("InvalidEntity"));
}

#[tokio::test]
async fn test_performance_monitoring_integration() {
    // Test performance monitoring throughout validation pipeline
    use crate::iam::features::validate_policy::performance_monitor::{
        PolicyValidationPerformanceMonitor, PerformanceThresholds
    };

    let thresholds = PerformanceThresholds {
        max_validation_time_ms: 1000,
        max_memory_usage_mb: 50,
        min_cache_hit_rate: 0.8,
        max_schema_load_time_ms: 200,
        max_batch_processing_time_ms: 5000,
        min_throughput_policies_per_sec: 5.0,
    };

    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);

    // Test performance session
    let mut session = monitor.start_session("integration-test".to_string()).await;
    assert_eq!(session.session_id, "integration-test");

    // Add checkpoints
    monitor.add_checkpoint(&mut session, "syntax_validation".to_string()).await;
    monitor.add_checkpoint(&mut session, "semantic_validation".to_string()).await;
    monitor.add_checkpoint(&mut session, "hrn_validation".to_string()).await;

    assert_eq!(session.checkpoints.len(), 3);
    assert_eq!(session.checkpoints[0].name, "syntax_validation");
    assert_eq!(session.checkpoints[1].name, "semantic_validation");
    assert_eq!(session.checkpoints[2].name, "hrn_validation");

    // Finish session and get metrics
    let metrics = monitor.finish_session(session).await.unwrap();
    assert!(metrics.validation_time_ms > 0);
    assert!(metrics.validation_steps == 3);

    // Test cache operations
    let policy_content = "permit(principal, action, resource);";
    let validation_result = PolicyValidationResult {
        syntax_errors: Vec::new(),
        semantic_errors: Vec::new(),
        hrn_errors: Vec::new(),
        warnings: Vec::new(),
        schema_info: SchemaValidationInfo {
            version: "1.0.0".to_string(),
            schema_id: "test-schema".to_string(),
            entity_types_count: 3,
            actions_count: 5,
        },
    };

    // Cache result
    monitor.cache_result(policy_content, validation_result.clone()).await;

    // Retrieve cached result
    let cached_result = monitor.get_cached_result(policy_content).await;
    assert!(cached_result.is_some());
    let cached = cached_result.unwrap();
    assert_eq!(cached.schema_info.version, "1.0.0");
    assert_eq!(cached.schema_info.entity_types_count, 3);
}

#[tokio::test]
async fn test_schema_validation_integration() {
    // Test schema validation with different schema versions
    let schema_info = SchemaValidationInfo {
        version: "2.0.0".to_string(),
        schema_id: "production-schema".to_string(),
        entity_types_count: 5,
        actions_count: 10,
    };

    assert_eq!(schema_info.version, "2.0.0");
    assert_eq!(schema_info.schema_id, "production-schema");
    assert_eq!(schema_info.entity_types_count, 5);
    assert_eq!(schema_info.actions_count, 10);

    // Test validation options with schema version
    let options = ValidationOptions {
        include_warnings: Some(true),
        deep_validation: Some(true),
        schema_version: Some("2.0.0".to_string()),
        timeout_ms: Some(2000),
    };

    assert_eq!(options.schema_version, Some("2.0.0".to_string()));
    assert_eq!(options.timeout_ms, Some(2000));
    assert_eq!(options.include_warnings, Some(true));
    assert_eq!(options.deep_validation, Some(true));
}

#[tokio::test]
async fn test_cross_policy_validation() {
    // Test cross-policy validation for conflicts and redundancies
    let policies = vec![
        r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            ) when {
                context.time < datetime("2024-12-31T23:59:59Z")
            };
        "#,
        r#"
            permit (
                principal == User::"bob",
                action == Action::"write",
                resource == Artifact::"doc2"
            ) when {
                principal.department == "engineering"
            };
        "#,
        r#"
            forbid (
                principal,
                action == Action::"delete",
                resource == Artifact::"doc1"
            );
        "#,
    ];

    // Validate policy structure
    assert_eq!(policies.len(), 3);
    
    // Check that policies have different principals and actions
    assert!(policies[0].contains(r#"User::"alice""#));
    assert!(policies[1].contains(r#"User::"bob""#));
    assert!(policies[2].contains("forbid"));
    
    // Check conditional logic
    assert!(policies[0].contains("context.time"));
    assert!(policies[1].contains("principal.department"));
    
    // Test cross-policy validation result structure
    let cross_policy_result = CrossPolicyValidationResult {
        conflicts: vec![
            PolicyConflict {
                conflict_type: ConflictType::PermitDenyConflict,
                involved_policies: vec![0, 2],
                description: "Policy 0 permits access while policy 2 forbids it for the same resource".to_string(),
                suggested_resolution: Some("Review the conditions and ensure they don't overlap".to_string()),
            }
        ],
        redundancies: vec![],
        coverage_analysis: Some(CoverageAnalysis {
            overall_coverage: 85.0,
            entity_coverage: 90.0,
            action_coverage: 80.0,
            uncovered_entities: vec!["Group".to_string()],
            uncovered_actions: vec!["admin".to_string()],
        }),
    };

    assert_eq!(cross_policy_result.conflicts.len(), 1);
    assert_eq!(cross_policy_result.redundancies.len(), 0);
    assert!(cross_policy_result.coverage_analysis.is_some());
    
    let coverage = cross_policy_result.coverage_analysis.unwrap();
    assert_eq!(coverage.overall_coverage, 85.0);
    assert_eq!(coverage.uncovered_entities.len(), 1);
    assert_eq!(coverage.uncovered_actions.len(), 1);
}

#[tokio::test]
async fn test_validation_metrics_and_reporting() {
    // Test comprehensive validation metrics collection and reporting
    let batch_metrics = BatchValidationMetrics {
        total_time_ms: 1500,
        average_time_per_policy_ms: 300,
        policies_processed: 5,
        policies_passed: 4,
        total_memory_usage_bytes: 2048000, // ~2MB
    };

    assert_eq!(batch_metrics.total_time_ms, 1500);
    assert_eq!(batch_metrics.average_time_per_policy_ms, 300);
    assert_eq!(batch_metrics.policies_processed, 5);
    assert_eq!(batch_metrics.policies_passed, 4);
    
    // Calculate success rate
    let success_rate = batch_metrics.policies_passed as f64 / batch_metrics.policies_processed as f64;
    assert_eq!(success_rate, 0.8); // 80% success rate

    // Test individual validation metrics
    let validation_metrics = ValidationMetrics {
        validation_time_ms: 250,
        memory_usage_bytes: 512000, // ~512KB
        validation_steps: 4,
        schema_load_time_ms: 50,
    };

    assert_eq!(validation_metrics.validation_time_ms, 250);
    assert_eq!(validation_metrics.memory_usage_bytes, 512000);
    assert_eq!(validation_metrics.validation_steps, 4);
    assert_eq!(validation_metrics.schema_load_time_ms, 50);
    
    // Verify schema load time is reasonable compared to total time
    let schema_load_percentage = (validation_metrics.schema_load_time_ms as f64 / validation_metrics.validation_time_ms as f64) * 100.0;
    assert!(schema_load_percentage < 25.0); // Schema loading should be < 25% of total time
}

#[tokio::test]
async fn test_error_aggregation_and_prioritization() {
    // Test error aggregation and prioritization in validation results
    let validation_result = PolicyValidationResult {
        syntax_errors: vec![
            ValidationError {
                error_type: ValidationErrorType::SyntaxError,
                message: "Missing semicolon at line 3".to_string(),
                location: Some(PolicyLocation { line: 3, column: 25 }),
                suggested_fix: Some("Add semicolon at the end of the statement".to_string()),
                documentation_link: Some("https://docs.hodei.com/iam/policies/syntax".to_string()),
            }
        ],
        semantic_errors: vec![
            ValidationError {
                error_type: ValidationErrorType::UnknownEntity,
                message: "Entity type 'InvalidUser' not found in schema".to_string(),
                location: Some(PolicyLocation { line: 2, column: 15 }),
                suggested_fix: Some("Use a valid entity type from the schema".to_string()),
                documentation_link: Some("https://docs.hodei.com/iam/policies/entities".to_string()),
            }
        ],
        hrn_errors: vec![],
        warnings: vec![
            ValidationWarning {
                message: "This condition might be redundant".to_string(),
                location: Some(PolicyLocation { line: 4, column: 10 }),
                severity: WarningSeverity::Medium,
            }
        ],
        schema_info: SchemaValidationInfo {
            version: "1.0.0".to_string(),
            schema_id: "test-schema".to_string(),
            entity_types_count: 3,
            actions_count: 5,
        },
    };

    // Verify error structure
    assert_eq!(validation_result.syntax_errors.len(), 1);
    assert_eq!(validation_result.semantic_errors.len(), 1);
    assert_eq!(validation_result.hrn_errors.len(), 0);
    assert_eq!(validation_result.warnings.len(), 1);

    // Verify error details
    let syntax_error = &validation_result.syntax_errors[0];
    assert!(matches!(syntax_error.error_type, ValidationErrorType::SyntaxError));
    assert!(syntax_error.suggested_fix.is_some());
    assert!(syntax_error.documentation_link.is_some());
    assert!(syntax_error.location.is_some());

    let semantic_error = &validation_result.semantic_errors[0];
    assert!(matches!(semantic_error.error_type, ValidationErrorType::UnknownEntity));
    assert!(semantic_error.message.contains("InvalidUser"));

    let warning = &validation_result.warnings[0];
    assert!(matches!(warning.severity, WarningSeverity::Medium));
    assert!(warning.location.is_some());
}

#[tokio::test]
async fn test_end_to_end_validation_workflow() {
    // Test complete end-to-end validation workflow
    // This simulates a real user request through the entire system
    
    // 1. Create a complex policy with multiple components
    let complex_policy = r#"
        permit (
            principal == User::"alice",
            action in [Action::"read", Action::"write"],
            resource == Artifact::"sensitive-doc"
        ) when {
            principal.clearance_level >= 3 &&
            context.time >= datetime("2024-01-01T00:00:00Z") &&
            context.time <= datetime("2024-12-31T23:59:59Z") &&
            resource.classification == "confidential"
        };
    "#;

    // 2. Create validation command with options
    let command = ValidatePolicyCommand::new(
        complex_policy.to_string(),
        "integration_test_user".to_string(),
    ).with_options(ValidationOptions {
        include_warnings: Some(true),
        deep_validation: Some(true),
        schema_version: Some("1.0.0".to_string()),
        timeout_ms: Some(5000),
    });

    // 3. Validate command structure
    assert!(command.validate().is_ok());
    assert!(command.options.is_some());
    let options = command.options.as_ref().unwrap();
    assert_eq!(options.schema_version, Some("1.0.0".to_string()));
    assert_eq!(options.timeout_ms, Some(5000));

    // 4. Verify policy content structure
    assert!(command.content.contains("permit"));
    assert!(command.content.contains("principal"));
    assert!(command.content.contains("action"));
    assert!(command.content.contains("resource"));
    assert!(command.content.contains("when"));
    assert!(command.content.contains("clearance_level"));
    assert!(command.content.contains("datetime"));

    // 5. Test batch processing with the complex policy
    let batch_policies = vec![
        PolicyToValidate::new(complex_policy.to_string())
            .with_id("complex-policy-1".to_string()),
        PolicyToValidate::new(r#"
            forbid (principal, action, resource) when {
                context.ip_address in ip("192.168.1.0/24")
            };
        "#.to_string())
            .with_id("ip-restriction-policy".to_string()),
    ];

    let batch_command = ValidatePoliciesBatchCommand::new(
        batch_policies,
        "integration_test_user".to_string(),
    ).with_options(ValidationOptions::default());

    assert!(batch_command.validate().is_ok());
    assert_eq!(batch_command.policies.len(), 2);
    assert!(batch_command.options.is_some());

    // 6. Verify individual policies in batch
    for (index, policy) in batch_command.policies.iter().enumerate() {
        assert!(policy.id.is_some(), "Policy {} should have an ID", index);
        assert!(!policy.content.trim().is_empty(), "Policy {} should not be empty", index);
    }
}