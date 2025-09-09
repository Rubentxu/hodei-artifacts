// crates/iam/tests/validate_policy_integration_test.rs

use iam::features::validate_policy::{
    ValidatePolicyDI, ValidatePolicyCommand, ValidatePoliciesBatchCommand, 
    PolicyToValidate, ValidationOptions, PolicyValidationMetadata
};
use iam::infrastructure::errors::IamError;
use std::collections::HashMap;

/// Integration tests for the validate_policy feature
/// These tests verify the complete flow from API to adapters

#[tokio::test]
async fn test_validate_policy_integration_valid_policy() {
    // Create the DI container with all real implementations
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Test with a valid Cedar policy
    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
        options: None,
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Validation should succeed for valid policy");

    let response = result.unwrap();
    assert!(response.is_valid, "Policy should be marked as valid");
    assert!(response.validation_result.syntax_errors.is_empty(), "Should have no syntax errors");
    assert!(!response.validation_id.is_empty(), "Should have validation ID");
    assert!(response.metrics.validation_time_ms > 0, "Should have recorded validation time");
}

#[tokio::test]
async fn test_validate_policy_integration_invalid_syntax() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Test with invalid syntax
    let command = ValidatePolicyCommand {
        content: "invalid policy syntax here".to_string(),
        options: None,
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Service should handle invalid syntax gracefully");

    let response = result.unwrap();
    assert!(!response.is_valid, "Policy should be marked as invalid");
    assert!(!response.validation_result.syntax_errors.is_empty(), "Should have syntax errors");
    
    let syntax_error = &response.validation_result.syntax_errors[0];
    assert_eq!(syntax_error.error_type, iam::features::validate_policy::ValidationErrorType::SyntaxError);
    assert!(!syntax_error.message.is_empty(), "Error should have a message");
}

#[tokio::test]
async fn test_validate_policy_integration_with_options() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let options = ValidationOptions {
        include_warnings: Some(true),
        deep_validation: Some(true),
        schema_version: Some("v1.0".to_string()),
        timeout_ms: Some(5000),
    };

    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
        options: Some(options),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Validation with options should succeed");

    let response = result.unwrap();
    assert!(response.is_valid, "Valid policy should pass with options");
    // Verify that options were applied (warnings might be included)
    assert!(response.metrics.validation_time_ms > 0);
}

#[tokio::test]
async fn test_validate_policy_integration_empty_content() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let command = ValidatePolicyCommand {
        content: "".to_string(),
        options: None,
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_err(), "Empty content should be rejected");

    match result.unwrap_err() {
        IamError::InvalidInput(msg) => {
            assert!(msg.contains("cannot be empty") || msg.contains("required"));
        }
        _ => panic!("Expected InvalidInput error for empty content"),
    }
}

#[tokio::test]
async fn test_validate_policy_integration_with_hrns() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Policy with HRNs
    let policy_with_hrn = r#"
        permit(
            principal == User::"hrn:hodei:iam:us-east-1:123456789012:user/alice",
            action == Action::"read",
            resource == Resource::"hrn:hodei:s3:us-east-1:123456789012:bucket/my-bucket"
        );
    "#;

    let command = ValidatePolicyCommand {
        content: policy_with_hrn.to_string(),
        options: None,
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Policy with HRNs should be processed");

    let response = result.unwrap();
    // The validity depends on whether the HRNs are properly formatted
    // At minimum, the service should process it without crashing
    assert!(!response.validation_id.is_empty());
}

#[tokio::test]
async fn test_validate_policy_integration_complex_policy() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // More complex policy with conditions
    let complex_policy = r#"
        permit(
            principal in Group::"admins",
            action in [Action::"read", Action::"write"],
            resource
        )
        when {
            context.ip_address.isIpv4() &&
            context.time_of_day > "09:00:00" &&
            context.time_of_day < "17:00:00"
        };
    "#;

    let command = ValidatePolicyCommand {
        content: complex_policy.to_string(),
        options: Some(ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: None,
            timeout_ms: Some(10000),
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Complex policy should be processed");

    let response = result.unwrap();
    // Complex policies might have semantic issues without proper schema
    // but should at least be syntactically processable
    assert!(!response.validation_id.is_empty());
    assert!(response.metrics.validation_time_ms > 0);
}

#[tokio::test]
async fn test_validate_policies_batch_integration() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let policies = vec![
        PolicyToValidate {
            id: Some("policy-1".to_string()),
            content: "permit(principal, action, resource);".to_string(),
            metadata: Some(PolicyValidationMetadata {
                name: Some("Test Policy 1".to_string()),
                description: Some("A simple permit policy".to_string()),
                tags: Some(vec!["test".to_string(), "permit".to_string()]),
                priority: Some(1),
            }),
        },
        PolicyToValidate {
            id: Some("policy-2".to_string()),
            content: "forbid(principal, action, resource);".to_string(),
            metadata: Some(PolicyValidationMetadata {
                name: Some("Test Policy 2".to_string()),
                description: Some("A simple forbid policy".to_string()),
                tags: Some(vec!["test".to_string(), "forbid".to_string()]),
                priority: Some(2),
            }),
        },
        PolicyToValidate {
            id: Some("policy-3".to_string()),
            content: "invalid syntax here".to_string(),
            metadata: None,
        },
    ];

    let batch_command = ValidatePoliciesBatchCommand {
        policies,
        options: Some(ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(false), // Faster for batch
            schema_version: None,
            timeout_ms: Some(15000),
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policies_batch(batch_command).await;
    assert!(result.is_ok(), "Batch validation should complete");

    let response = result.unwrap();
    assert_eq!(response.individual_results.len(), 3, "Should have results for all 3 policies");
    assert!(!response.overall_valid, "Overall should be invalid due to policy-3");
    
    // Check individual results
    let policy1_result = &response.individual_results[0];
    assert!(policy1_result.is_valid, "Policy 1 should be valid");
    assert_eq!(policy1_result.policy_id, Some("policy-1".to_string()));

    let policy2_result = &response.individual_results[1];
    assert!(policy2_result.is_valid, "Policy 2 should be valid");
    assert_eq!(policy2_result.policy_id, Some("policy-2".to_string()));

    let policy3_result = &response.individual_results[2];
    assert!(!policy3_result.is_valid, "Policy 3 should be invalid");
    assert_eq!(policy3_result.policy_id, Some("policy-3".to_string()));
    assert!(!policy3_result.validation_result.syntax_errors.is_empty());

    // Check batch metrics
    assert_eq!(response.batch_metrics.total_policies, 3);
    assert_eq!(response.batch_metrics.valid_policies, 2);
    assert_eq!(response.batch_metrics.invalid_policies, 1);
    assert!(response.batch_metrics.total_time_ms > 0);
    assert!(!response.batch_id.is_empty());
}

#[tokio::test]
async fn test_validate_policies_batch_integration_empty() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let batch_command = ValidatePoliciesBatchCommand {
        policies: vec![],
        options: None,
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policies_batch(batch_command).await;
    assert!(result.is_err(), "Empty batch should be rejected");

    match result.unwrap_err() {
        IamError::InvalidInput(msg) => {
            assert!(msg.contains("At least one policy") || msg.contains("empty"));
        }
        _ => panic!("Expected InvalidInput error for empty batch"),
    }
}

#[tokio::test]
async fn test_validate_policies_batch_integration_large_batch() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Create a larger batch to test performance and scalability
    let mut policies = Vec::new();
    for i in 0..50 {
        policies.push(PolicyToValidate {
            id: Some(format!("policy-{}", i)),
            content: format!("permit(principal, action, resource{});", i),
            metadata: Some(PolicyValidationMetadata {
                name: Some(format!("Generated Policy {}", i)),
                description: Some(format!("Auto-generated policy number {}", i)),
                tags: Some(vec!["generated".to_string(), "test".to_string()]),
                priority: Some(i as u32),
            }),
        });
    }

    let batch_command = ValidatePoliciesBatchCommand {
        policies,
        options: Some(ValidationOptions {
            include_warnings: Some(false), // Disable warnings for performance
            deep_validation: Some(false),
            schema_version: None,
            timeout_ms: Some(30000), // Longer timeout for large batch
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policies_batch(batch_command).await;
    assert!(result.is_ok(), "Large batch validation should complete");

    let response = result.unwrap();
    assert_eq!(response.individual_results.len(), 50, "Should have results for all 50 policies");
    assert!(response.overall_valid, "All generated policies should be valid");
    assert_eq!(response.batch_metrics.total_policies, 50);
    assert_eq!(response.batch_metrics.valid_policies, 50);
    assert_eq!(response.batch_metrics.invalid_policies, 0);
    
    // Performance check - should complete in reasonable time
    assert!(response.batch_metrics.total_time_ms < 30000, "Should complete within timeout");
}

#[tokio::test]
async fn test_validate_policy_integration_cross_policy_analysis() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Create policies that might have conflicts
    let policies = vec![
        PolicyToValidate {
            id: Some("permit-policy".to_string()),
            content: "permit(principal, action, resource);".to_string(),
            metadata: None,
        },
        PolicyToValidate {
            id: Some("forbid-policy".to_string()),
            content: "forbid(principal, action, resource);".to_string(),
            metadata: None,
        },
    ];

    let batch_command = ValidatePoliciesBatchCommand {
        policies,
        options: Some(ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true), // Enable deep validation for cross-policy analysis
            schema_version: None,
            timeout_ms: Some(10000),
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policies_batch(batch_command).await;
    assert!(result.is_ok(), "Cross-policy analysis should complete");

    let response = result.unwrap();
    assert_eq!(response.individual_results.len(), 2);
    
    // Check if cross-policy validation detected conflicts
    if let Some(cross_policy_result) = &response.cross_policy_validation {
        // Might detect conflicts between permit and forbid policies
        println!("Conflicts detected: {}", cross_policy_result.conflicts.len());
        println!("Redundancies detected: {}", cross_policy_result.redundancies.len());
    }
}

#[tokio::test]
async fn test_validate_policy_integration_performance_metrics() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource) when context.authenticated == true;".to_string(),
        options: Some(ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: None,
            timeout_ms: Some(5000),
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    assert!(result.is_ok(), "Validation should succeed");

    let response = result.unwrap();
    
    // Verify that performance metrics are collected
    assert!(response.metrics.validation_time_ms > 0, "Should record total validation time");
    assert!(response.metrics.syntax_validation_time_ms >= 0, "Should record syntax validation time");
    assert!(response.metrics.semantic_validation_time_ms >= 0, "Should record semantic validation time");
    assert!(response.metrics.hrn_validation_time_ms >= 0, "Should record HRN validation time");
    
    // Total time should be at least the sum of individual components
    let component_sum = response.metrics.syntax_validation_time_ms + 
                       response.metrics.semantic_validation_time_ms + 
                       response.metrics.hrn_validation_time_ms;
    assert!(response.metrics.validation_time_ms >= component_sum, 
           "Total time should be at least the sum of components");

    if let Some(memory_usage) = response.metrics.memory_usage_bytes {
        assert!(memory_usage > 0, "Memory usage should be positive if recorded");
    }
}

#[tokio::test]
async fn test_validate_policy_integration_error_handling() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Test various error conditions
    let test_cases = vec![
        ("", "Empty content"),
        ("   ", "Whitespace only content"),
        ("permit(", "Incomplete syntax"),
        ("permit(principal, action, resource) when;", "Incomplete condition"),
        ("forbid(principal == User::\"malformed-hrn\", action, resource);", "Malformed HRN"),
    ];

    for (content, description) in test_cases {
        let command = ValidatePolicyCommand {
            content: content.to_string(),
            options: None,
            requested_by: "integration_test".to_string(),
        };

        let result = validation_service.validate_policy(command).await;
        
        if content.trim().is_empty() {
            // Empty content should be rejected at the service level
            assert!(result.is_err(), "Empty content should be rejected: {}", description);
        } else {
            // Other errors should be handled gracefully with validation response
            assert!(result.is_ok(), "Service should handle errors gracefully: {}", description);
            let response = result.unwrap();
            assert!(!response.is_valid, "Invalid content should be marked as invalid: {}", description);
            assert!(!response.validation_result.syntax_errors.is_empty() || 
                   !response.validation_result.semantic_errors.is_empty() ||
                   !response.validation_result.hrn_errors.is_empty(), 
                   "Should have some validation errors: {}", description);
        }
    }
}

#[tokio::test]
async fn test_validate_policy_integration_timeout_handling() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    // Test with very short timeout
    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
        options: Some(ValidationOptions {
            include_warnings: Some(true),
            deep_validation: Some(true),
            schema_version: None,
            timeout_ms: Some(1), // Very short timeout
        }),
        requested_by: "integration_test".to_string(),
    };

    let result = validation_service.validate_policy(command).await;
    
    // The service should either complete quickly or handle timeout gracefully
    match result {
        Ok(response) => {
            // If it completed, it should be valid
            assert!(!response.validation_id.is_empty());
        }
        Err(IamError::ValidationError(msg)) => {
            // If it timed out, should have appropriate error message
            assert!(msg.contains("timeout") || msg.contains("time"));
        }
        Err(e) => {
            panic!("Unexpected error type for timeout test: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_validate_policy_integration_schema_versions() {
    let di = ValidatePolicyDI::new().expect("Failed to create DI container");
    let validation_service = di.get_validation_service();

    let schema_versions = vec!["v1.0", "v2.0", "latest", "invalid-version"];

    for version in schema_versions {
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: Some(ValidationOptions {
                include_warnings: Some(false),
                deep_validation: Some(true),
                schema_version: Some(version.to_string()),
                timeout_ms: Some(5000),
            }),
            requested_by: "integration_test".to_string(),
        };

        let result = validation_service.validate_policy(command).await;
        
        // Service should handle all schema versions gracefully
        assert!(result.is_ok(), "Should handle schema version '{}' gracefully", version);
        
        let response = result.unwrap();
        assert!(!response.validation_id.is_empty());
        
        // For invalid schema versions, might have warnings or use default
        if version == "invalid-version" {
            // Should either use default schema or include warnings about invalid version
            println!("Response for invalid schema version: valid={}", response.is_valid);
        }
    }
}

/// Helper function to create a test DI container with specific configurations
fn create_test_di_with_config() -> Result<ValidatePolicyDI, IamError> {
    // This could be extended to create DI with specific test configurations
    ValidatePolicyDI::new()
}

#[tokio::test]
async fn test_validate_policy_integration_di_container() {
    // Test that the DI container can be created and used multiple times
    let di1 = create_test_di_with_config().expect("First DI creation should succeed");
    let di2 = create_test_di_with_config().expect("Second DI creation should succeed");

    let service1 = di1.get_validation_service();
    let service2 = di2.get_validation_service();

    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
        options: None,
        requested_by: "integration_test".to_string(),
    };

    // Both services should work independently
    let result1 = service1.validate_policy(command.clone()).await;
    let result2 = service2.validate_policy(command).await;

    assert!(result1.is_ok(), "First service should work");
    assert!(result2.is_ok(), "Second service should work");

    let response1 = result1.unwrap();
    let response2 = result2.unwrap();

    assert!(response1.is_valid, "First validation should succeed");
    assert!(response2.is_valid, "Second validation should succeed");
    assert_ne!(response1.validation_id, response2.validation_id, "Should have different validation IDs");
}