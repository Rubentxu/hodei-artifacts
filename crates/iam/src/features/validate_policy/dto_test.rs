// crates/iam/src/features/validate_policy/dto_test.rs

#[cfg(test)]
mod tests {
    use super::super::dto::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_policy_command_creation() {
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "test-user".to_string(),
        };

        assert_eq!(command.content, "permit(principal, action, resource);");
        assert_eq!(command.requested_by, "test-user");
        assert!(command.options.is_none());
    }

    #[test]
    fn test_validate_policy_command_with_options() {
        let options = ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: Some("v1.0".to_string()),
            timeout_ms: Some(5000),
        };

        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: Some(options),
            requested_by: "test-user".to_string(),
        };

        assert!(command.options.is_some());
        let opts = command.options.unwrap();
        assert_eq!(opts.include_warnings, Some(true));
        assert_eq!(opts.deep_validation, Some(true));
        assert_eq!(opts.schema_version, Some("v1.0".to_string()));
        assert_eq!(opts.timeout_ms, Some(5000));
    }

    #[test]
    fn test_validate_policies_batch_command() {
        let policy1 = PolicyToValidate {
            id: Some("policy-1".to_string()),
            content: "permit(principal, action, resource);".to_string(),
            metadata: None,
        };

        let policy2 = PolicyToValidate {
            id: Some("policy-2".to_string()),
            content: "forbid(principal, action, resource);".to_string(),
            metadata: Some(PolicyValidationMetadata {
                name: Some("Test Policy 2".to_string()),
                description: Some("A test forbid policy".to_string()),
                tags: Some(vec!["test".to_string(), "forbid".to_string()]),
                priority: Some(10),
            }),
        };

        let batch_command = ValidatePoliciesBatchCommand {
            policies: vec![policy1, policy2],
            options: None,
            requested_by: "batch-user".to_string(),
        };

        assert_eq!(batch_command.policies.len(), 2);
        assert_eq!(batch_command.requested_by, "batch-user");
        assert_eq!(batch_command.policies[0].id, Some("policy-1".to_string()));
        assert_eq!(batch_command.policies[1].id, Some("policy-2".to_string()));
        assert!(batch_command.policies[0].metadata.is_none());
        assert!(batch_command.policies[1].metadata.is_some());
    }

    #[test]
    fn test_policy_to_validate_with_metadata() {
        let metadata = PolicyValidationMetadata {
            name: Some("Test Policy".to_string()),
            description: Some("A comprehensive test policy".to_string()),
            tags: Some(vec!["test".to_string(), "validation".to_string()]),
            priority: Some(5),
        };

        let policy = PolicyToValidate {
            id: Some("test-policy-123".to_string()),
            content: "permit(principal, action, resource) when condition;".to_string(),
            metadata: Some(metadata),
        };

        assert_eq!(policy.id, Some("test-policy-123".to_string()));
        assert!(policy.content.contains("when condition"));
        
        let meta = policy.metadata.unwrap();
        assert_eq!(meta.name, Some("Test Policy".to_string()));
        assert_eq!(meta.description, Some("A comprehensive test policy".to_string()));
        assert_eq!(meta.tags, Some(vec!["test".to_string(), "validation".to_string()]));
        assert_eq!(meta.priority, Some(5));
    }

    #[test]
    fn test_validation_options_defaults() {
        let options = ValidationOptions {
            include_warnings: None,
            deep_validation: None,
            schema_version: None,
            timeout_ms: None,
        };

        // Test that None values are handled properly
        assert!(options.include_warnings.is_none());
        assert!(options.deep_validation.is_none());
        assert!(options.schema_version.is_none());
        assert!(options.timeout_ms.is_none());
    }

    #[test]
    fn test_validate_policy_response_creation() {
        let validation_result = PolicyValidationResult {
            syntax_errors: vec![],
            semantic_errors: vec![],
            hrn_errors: vec![],
            warnings: vec![],
            cross_policy_issues: vec![],
            schema_info: Some(SchemaValidationInfo {
                version: "v1.0".to_string(),
                entities_validated: vec!["User".to_string(), "Resource".to_string()],
                actions_validated: vec!["read".to_string(), "write".to_string()],
            }),
        };

        let metrics = ValidationMetrics {
            validation_time_ms: 150,
            syntax_validation_time_ms: 50,
            semantic_validation_time_ms: 75,
            hrn_validation_time_ms: 25,
            memory_usage_bytes: Some(1024),
        };

        let response = ValidatePolicyResponse {
            is_valid: true,
            validation_result,
            metrics,
            validation_id: "val-123".to_string(),
        };

        assert!(response.is_valid);
        assert_eq!(response.validation_id, "val-123");
        assert_eq!(response.metrics.validation_time_ms, 150);
        assert!(response.validation_result.syntax_errors.is_empty());
        assert!(response.validation_result.semantic_errors.is_empty());
        assert!(response.validation_result.hrn_errors.is_empty());
    }

    #[test]
    fn test_validate_policy_response_with_errors() {
        let syntax_error = ValidationError {
            error_type: ValidationErrorType::SyntaxError,
            message: "Invalid syntax at line 1".to_string(),
            location: Some(PolicyLocation {
                line: 1,
                column: 10,
                offset: Some(9),
            }),
            suggested_fix: Some("Check parentheses matching".to_string()),
            documentation_link: Some("https://docs.cedarpolicy.com/syntax".to_string()),
        };

        let semantic_error = ValidationError {
            error_type: ValidationErrorType::SemanticError,
            message: "Unknown entity type 'InvalidEntity'".to_string(),
            location: Some(PolicyLocation {
                line: 1,
                column: 20,
                offset: Some(19),
            }),
            suggested_fix: Some("Use a valid entity type from the schema".to_string()),
            documentation_link: Some("https://docs.cedarpolicy.com/entities".to_string()),
        };

        let validation_result = PolicyValidationResult {
            syntax_errors: vec![syntax_error],
            semantic_errors: vec![semantic_error],
            hrn_errors: vec![],
            warnings: vec![],
            cross_policy_issues: vec![],
            schema_info: None,
        };

        let response = ValidatePolicyResponse {
            is_valid: false,
            validation_result,
            metrics: ValidationMetrics {
                validation_time_ms: 200,
                syntax_validation_time_ms: 100,
                semantic_validation_time_ms: 100,
                hrn_validation_time_ms: 0,
                memory_usage_bytes: Some(2048),
            },
            validation_id: "val-456".to_string(),
        };

        assert!(!response.is_valid);
        assert_eq!(response.validation_result.syntax_errors.len(), 1);
        assert_eq!(response.validation_result.semantic_errors.len(), 1);
        assert_eq!(response.validation_result.syntax_errors[0].error_type, ValidationErrorType::SyntaxError);
        assert_eq!(response.validation_result.semantic_errors[0].error_type, ValidationErrorType::SemanticError);
    }

    #[test]
    fn test_batch_validation_response() {
        let individual_result1 = IndividualValidationResult {
            index: 0,
            policy_id: Some("policy-1".to_string()),
            is_valid: true,
            validation_result: PolicyValidationResult {
                syntax_errors: vec![],
                semantic_errors: vec![],
                hrn_errors: vec![],
                warnings: vec![],
                cross_policy_issues: vec![],
                schema_info: None,
            },
        };

        let individual_result2 = IndividualValidationResult {
            index: 1,
            policy_id: Some("policy-2".to_string()),
            is_valid: false,
            validation_result: PolicyValidationResult {
                syntax_errors: vec![ValidationError {
                    error_type: ValidationErrorType::SyntaxError,
                    message: "Syntax error in policy 2".to_string(),
                    location: None,
                    suggested_fix: None,
                    documentation_link: None,
                }],
                semantic_errors: vec![],
                hrn_errors: vec![],
                warnings: vec![],
                cross_policy_issues: vec![],
                schema_info: None,
            },
        };

        let cross_policy_result = CrossPolicyValidationResult {
            conflicts: vec![],
            redundancies: vec![],
            coverage_gaps: vec![],
        };

        let batch_response = ValidatePoliciesBatchResponse {
            overall_valid: false,
            individual_results: vec![individual_result1, individual_result2],
            cross_policy_validation: cross_policy_result,
            batch_metrics: BatchValidationMetrics {
                total_time_ms: 500,
                individual_validation_time_ms: 400,
                cross_policy_validation_time_ms: 100,
                total_policies: 2,
                valid_policies: 1,
                invalid_policies: 1,
            },
            batch_id: "batch-789".to_string(),
        };

        assert!(!batch_response.overall_valid);
        assert_eq!(batch_response.individual_results.len(), 2);
        assert!(batch_response.individual_results[0].is_valid);
        assert!(!batch_response.individual_results[1].is_valid);
        assert_eq!(batch_response.batch_metrics.total_policies, 2);
        assert_eq!(batch_response.batch_metrics.valid_policies, 1);
        assert_eq!(batch_response.batch_metrics.invalid_policies, 1);
    }

    #[test]
    fn test_validation_error_types() {
        let error_types = vec![
            ValidationErrorType::SyntaxError,
            ValidationErrorType::SemanticError,
            ValidationErrorType::HrnError,
            ValidationErrorType::SchemaError,
            ValidationErrorType::CrossPolicyError,
        ];

        assert_eq!(error_types.len(), 5);
        
        // Test that error types can be compared
        assert_eq!(ValidationErrorType::SyntaxError, ValidationErrorType::SyntaxError);
        assert_ne!(ValidationErrorType::SyntaxError, ValidationErrorType::SemanticError);
    }

    #[test]
    fn test_validation_warning() {
        let warning = ValidationWarning {
            message: "Policy is overly permissive".to_string(),
            warning_type: WarningType::OverlyPermissive,
            location: Some(PolicyLocation {
                line: 1,
                column: 1,
                offset: Some(0),
            }),
            suggestion: Some("Consider adding more specific conditions".to_string()),
        };

        assert_eq!(warning.message, "Policy is overly permissive");
        assert_eq!(warning.warning_type, WarningType::OverlyPermissive);
        assert!(warning.location.is_some());
        assert!(warning.suggestion.is_some());
    }

    #[test]
    fn test_warning_types() {
        let warning_types = vec![
            WarningType::OverlyPermissive,
            WarningType::UnusedCondition,
            WarningType::DeprecatedSyntax,
            WarningType::PerformanceImpact,
            WarningType::BestPractice,
        ];

        assert_eq!(warning_types.len(), 5);
        assert_eq!(WarningType::OverlyPermissive, WarningType::OverlyPermissive);
        assert_ne!(WarningType::OverlyPermissive, WarningType::UnusedCondition);
    }

    #[test]
    fn test_policy_location() {
        let location = PolicyLocation {
            line: 5,
            column: 15,
            offset: Some(120),
        };

        assert_eq!(location.line, 5);
        assert_eq!(location.column, 15);
        assert_eq!(location.offset, Some(120));
    }

    #[test]
    fn test_schema_validation_info() {
        let schema_info = SchemaValidationInfo {
            version: "v2.1".to_string(),
            entities_validated: vec!["User".to_string(), "Group".to_string(), "Resource".to_string()],
            actions_validated: vec!["read".to_string(), "write".to_string(), "delete".to_string()],
        };

        assert_eq!(schema_info.version, "v2.1");
        assert_eq!(schema_info.entities_validated.len(), 3);
        assert_eq!(schema_info.actions_validated.len(), 3);
        assert!(schema_info.entities_validated.contains(&"User".to_string()));
        assert!(schema_info.actions_validated.contains(&"read".to_string()));
    }

    #[test]
    fn test_validation_metrics() {
        let metrics = ValidationMetrics {
            validation_time_ms: 1000,
            syntax_validation_time_ms: 200,
            semantic_validation_time_ms: 600,
            hrn_validation_time_ms: 200,
            memory_usage_bytes: Some(4096),
        };

        assert_eq!(metrics.validation_time_ms, 1000);
        assert_eq!(metrics.syntax_validation_time_ms, 200);
        assert_eq!(metrics.semantic_validation_time_ms, 600);
        assert_eq!(metrics.hrn_validation_time_ms, 200);
        assert_eq!(metrics.memory_usage_bytes, Some(4096));

        // Test that individual times sum up reasonably
        let individual_sum = metrics.syntax_validation_time_ms + 
                           metrics.semantic_validation_time_ms + 
                           metrics.hrn_validation_time_ms;
        assert_eq!(individual_sum, 1000);
    }

    #[test]
    fn test_batch_validation_metrics() {
        let batch_metrics = BatchValidationMetrics {
            total_time_ms: 2000,
            individual_validation_time_ms: 1500,
            cross_policy_validation_time_ms: 500,
            total_policies: 10,
            valid_policies: 8,
            invalid_policies: 2,
        };

        assert_eq!(batch_metrics.total_time_ms, 2000);
        assert_eq!(batch_metrics.individual_validation_time_ms, 1500);
        assert_eq!(batch_metrics.cross_policy_validation_time_ms, 500);
        assert_eq!(batch_metrics.total_policies, 10);
        assert_eq!(batch_metrics.valid_policies, 8);
        assert_eq!(batch_metrics.invalid_policies, 2);

        // Test that valid + invalid = total
        assert_eq!(batch_metrics.valid_policies + batch_metrics.invalid_policies, batch_metrics.total_policies);
    }

    #[test]
    fn test_cross_policy_validation_result() {
        let conflict = PolicyConflict {
            conflict_type: ConflictType::DirectContradiction,
            conflicting_policies: vec![0, 1],
            description: "Policy 0 permits what Policy 1 forbids".to_string(),
            severity: ConflictSeverity::High,
            suggested_resolution: Some("Review policy precedence".to_string()),
        };

        let redundancy = PolicyRedundancy {
            redundant_policies: vec![2, 3],
            reason: "Policies have identical effects".to_string(),
            confidence: 0.95,
        };

        let coverage_gap = CoverageAnalysis {
            overall_coverage: 85.5,
            uncovered_entities: vec!["SpecialResource".to_string()],
            uncovered_actions: vec!["admin_action".to_string()],
            coverage_by_entity: {
                let mut map = HashMap::new();
                map.insert("User".to_string(), 100.0);
                map.insert("Resource".to_string(), 90.0);
                map.insert("SpecialResource".to_string(), 0.0);
                map
            },
        };

        let cross_policy_result = CrossPolicyValidationResult {
            conflicts: vec![conflict],
            redundancies: vec![redundancy],
            coverage_gaps: vec![coverage_gap],
        };

        assert_eq!(cross_policy_result.conflicts.len(), 1);
        assert_eq!(cross_policy_result.redundancies.len(), 1);
        assert_eq!(cross_policy_result.coverage_gaps.len(), 1);

        let conflict = &cross_policy_result.conflicts[0];
        assert_eq!(conflict.conflict_type, ConflictType::DirectContradiction);
        assert_eq!(conflict.severity, ConflictSeverity::High);

        let redundancy = &cross_policy_result.redundancies[0];
        assert_eq!(redundancy.confidence, 0.95);

        let coverage = &cross_policy_result.coverage_gaps[0];
        assert_eq!(coverage.overall_coverage, 85.5);
        assert_eq!(coverage.uncovered_entities.len(), 1);
        assert_eq!(coverage.uncovered_actions.len(), 1);
    }

    #[test]
    fn test_conflict_types_and_severities() {
        let conflict_types = vec![
            ConflictType::DirectContradiction,
            ConflictType::OverlappingPermissions,
            ConflictType::AmbiguousPrecedence,
            ConflictType::CircularDependency,
        ];

        let severities = vec![
            ConflictSeverity::Critical,
            ConflictSeverity::High,
            ConflictSeverity::Medium,
            ConflictSeverity::Low,
        ];

        assert_eq!(conflict_types.len(), 4);
        assert_eq!(severities.len(), 4);

        // Test ordering implications
        assert_ne!(ConflictSeverity::Critical, ConflictSeverity::High);
        assert_ne!(ConflictType::DirectContradiction, ConflictType::OverlappingPermissions);
    }

    #[test]
    fn test_serde_serialization() {
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: Some(ValidationOptions {
                include_warnings: Some(true),
                deep_validation: Some(false),
                schema_version: None,
                timeout_ms: Some(1000),
            }),
            requested_by: "test-user".to_string(),
        };

        // Test that the command can be serialized to JSON
        let json = serde_json::to_string(&command).expect("Should serialize to JSON");
        assert!(json.contains("permit(principal, action, resource)"));
        assert!(json.contains("test-user"));

        // Test that it can be deserialized back
        let deserialized: ValidatePolicyCommand = serde_json::from_str(&json).expect("Should deserialize from JSON");
        assert_eq!(deserialized.content, command.content);
        assert_eq!(deserialized.requested_by, command.requested_by);
    }
}