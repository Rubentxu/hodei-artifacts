//! Unit tests for policy evaluation use case
//! 
//! This module provides comprehensive unit tests for the policy evaluation feature.


use crate::features::evaluate_policy::mocks::*;
use crate::features::evaluate_policy::*;
use std::sync::Arc;

/// Test suite for policy evaluation use case
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::str::FromStr;
    use shared::hrn::PolicyId;
    use tracing_test::traced_test;
    use crate::features::evaluate_policy::di::PolicyEvaluationContainer;

    /// Test successful policy evaluation
    #[tokio::test]
    #[traced_test]
    async fn test_evaluate_policy_success() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300, // 5 minute TTL
        );

        // Create test data
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Policy evaluation should succeed");
        let response = result.unwrap();
        assert!(response.decision.allowed, "Decision should be allowed");
        assert_eq!(response.policy_id, PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap());
        assert!(response.metrics.evaluation_time_ms > 0);
        assert!(!logs_contain("error"), "Should not contain error logs");
    }

    /// Test policy evaluation with non-existent policy
    #[tokio::test]
    #[traced_test]
    async fn test_evaluate_policy_not_found() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create request for non-existent policy
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/non-existent-policy").unwrap(),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_err(), "Policy evaluation should fail");
        let error = result.unwrap_err();
        assert!(matches!(error, EvaluatePolicyError::PolicyNotFound(_)));
        assert!(logs_contain("Policy not found"), "Should log policy not found error");
    }

    /// Test policy evaluation with cache hit
    #[tokio::test]
    #[traced_test]
    async fn test_evaluate_policy_cache_hit() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let cache = Arc::new(MockPolicyCache::new());
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            cache.clone(),
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Cache a result
        let cache_key = "test_cache_key";
        let cached_result = CachedEvaluationResult {
            decision: PolicyDecision {
                allowed: true,
                reasons: vec!["Cached decision".to_string()],
                obligations: vec![],
                advice: vec![],
            },
            cached_at: chrono::Utc::now(),
            ttl_seconds: 300,
        };
        cache.store(cache_key.to_string(), cached_result).await;

        // Create test data that would hit the cache
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::new("test-policy"),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Mock the cache to return the cached result
        cache.set_should_hit(true).await;

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Policy evaluation should succeed");
        let response = result.unwrap();
        assert!(response.decision.allowed, "Decision should be allowed from cache");
        assert_eq!(response.metrics.cache_hit_rate, 1.0, "Cache hit rate should be 1.0");
        assert!(logs_contain("Cache hit for policy evaluation"), "Should log cache hit");
    }

    /// Test policy evaluation with cache miss
    #[tokio::test]
    #[traced_test]
    async fn test_evaluate_policy_cache_miss() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let cache = Arc::new(MockPolicyCache::new());
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            cache.clone(),
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Mock cache miss
        cache.set_should_hit(false).await;

        // Create test data
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::new("test-policy"),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Policy evaluation should succeed");
        let response = result.unwrap();
        assert_eq!(response.metrics.cache_hit_rate, 0.0, "Cache hit rate should be 0.0");
        assert!(logs_contain("Cache miss, proceeding with full evaluation"), "Should log cache miss");
    }

    /// Test batch policy evaluation
    #[tokio::test]
    #[traced_test]
    async fn test_batch_evaluate_policies() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create batch request
        let requests = vec![
            EvaluatePolicyRequest {
                policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/policy-1").unwrap(),
                context: TestDataBuilder::sample_evaluation_context(),
            },
            EvaluatePolicyRequest {
                policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/policy-2").unwrap(),
                context: TestDataBuilder::sample_evaluation_context(),
            },
        ];

        let batch_request = BatchEvaluatePolicyRequest {
            requests: requests.clone(),
            parallel: false,
        };

        // Execute
        let result = use_case.batch_evaluate_policies(batch_request).await;

        // Verify
        assert!(result.is_ok(), "Batch evaluation should succeed");
        let response = result.unwrap();
        assert_eq!(response.responses.len(), 2, "Should have 2 responses");
        assert_eq!(response.batch_metrics.successful_evaluations, 2, "Should have 2 successful evaluations");
        assert_eq!(response.batch_metrics.failed_evaluations, 0, "Should have 0 failed evaluations");
        assert!(response.batch_metrics.total_time_ms > 0, "Total time should be positive");
        assert!(logs_contain("Batch policy evaluation completed"), "Should log batch completion");
    }

    /// Test batch policy evaluation with parallel execution
    #[tokio::test]
    #[traced_test]
    async fn test_batch_evaluate_policies_parallel() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create batch request with policies that have different evaluation times
        let requests = vec![
            EvaluatePolicyRequest {
                policy_id: PolicyId::new("policy-1"),
                context: TestDataBuilder::sample_evaluation_context(),
            },
            EvaluatePolicyRequest {
                policy_id: PolicyId::new("policy-2"),
                context: TestDataBuilder::sample_evaluation_context(),
            },
        ];

        let batch_request = BatchEvaluatePolicyRequest {
            requests: requests.clone(),
            parallel: true,
        };

        // Execute
        let result = use_case.batch_evaluate_policies(batch_request).await;

        // Verify
        assert!(result.is_ok(), "Batch evaluation should succeed");
        let response = result.unwrap();
        assert_eq!(response.responses.len(), 2, "Should have 2 responses");
        assert_eq!(response.batch_metrics.successful_evaluations, 2, "Should have 2 successful evaluations");
        assert!(logs_contain("Batch policy evaluation completed"), "Should log batch completion");
    }

    /// Test policy validation
    #[tokio::test]
    #[traced_test]
    async fn test_validate_policy_valid() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Valid policy content
        let policy_content = r#"
            permit(principal, action, resource)
            when {
                principal.department == resource.department &&
                action in ["read", "write"]
            };
        "#;

        // Execute
        let result = use_case.validate_policy(policy_content).await;

        // Verify
        assert!(result.is_ok(), "Policy validation should succeed");
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid, "Policy should be valid");
        assert!(validation_result.validation_time_ms > 0, "Validation time should be positive");
        assert!(!validation_result.compilation_hash.is_empty(), "Should have compilation hash");
        assert!(logs_contain("Policy validation completed successfully"), "Should log successful validation");
    }

    /// Test policy validation with invalid syntax
    #[tokio::test]
    #[traced_test]
    async fn test_validate_policy_invalid_syntax() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Invalid policy content with syntax error
        let policy_content = "invalid syntax here";

        // Execute
        let result = use_case.validate_policy(policy_content).await;

        // Verify
        assert!(result.is_err(), "Policy validation should fail");
        let error = result.unwrap_err();
        assert!(matches!(error, EvaluatePolicyError::PolicyCompilationError(_)));
        assert!(logs_contain("Policy compilation error"), "Should log compilation error");
    }

    /// Test finding applicable policies
    #[tokio::test]
    #[traced_test]
    async fn test_find_applicable_policies() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create query
        let query = FindApplicablePoliciesQuery {
            principal: "user::test-user".to_string(),
            action: "read".to_string(),
            resource: "resource::test-resource".to_string(),
            organization_id: Some("test-org".to_string()),
            environment: Some(HashMap::new()),
        };

        // Execute
        let result = use_case.find_applicable_policies(query).await;

        // Verify
        assert!(result.is_ok(), "Finding applicable policies should succeed");
        let response = result.unwrap();
        assert!(response.search_metrics.search_time_ms > 0, "Search time should be positive");
        assert!(logs_contain("Found applicable policies"), "Should log policy search");
    }

    /// Test getting evaluation statistics
    #[tokio::test]
    #[traced_test]
    async fn test_get_evaluation_statistics() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Execute
        let result = use_case.get_evaluation_statistics().await;

        // Verify
        assert!(result.is_ok(), "Getting statistics should succeed");
        let stats = result.unwrap();
        assert!(stats.calculated_at > chrono::Utc::now() - chrono::Duration::seconds(60), "Should be recent");
        assert!(logs_contain("Retrieved evaluation statistics"), "Should log statistics retrieval");
    }

    /// Test error handling for policy evaluation
    #[tokio::test]
    #[traced_test]
    async fn test_error_handling() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        
        // Configure mock to fail
        if let Some(engine) = container.cedar_engine.as_any().downcast_ref::<MockCedarEngine>() {
            engine.set_should_succeed(false).await;
        }

        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create test data
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::new("test-policy"),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Even failed evaluation should return a response");
        let response = result.unwrap();
        assert!(!response.decision.allowed, "Failed evaluation should deny access");
        assert!(logs_contain("Policy evaluation completed"), "Should still log completion");
    }

    /// Test performance metrics recording
    #[tokio::test]
    #[traced_test]
    async fn test_performance_metrics() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor.clone(),
            container.event_publisher,
            300,
        );

        // Create test data
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::new("test-policy"),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Policy evaluation should succeed");

        // Check if performance monitor recorded the evaluation
        let monitor = container.performance_monitor.as_any().downcast_ref::<MockPerformanceMonitor>().unwrap();
        let evaluations = monitor.get_evaluations().await;
        assert!(!evaluations.is_empty(), "Should have recorded evaluation metrics");
        
        let metrics = &evaluations[0];
        assert!(metrics.evaluation_time_ms > 0, "Should have recorded evaluation time");
        assert_eq!(metrics.policies_evaluated, 1, "Should have evaluated 1 policy");
        assert!(logs_contain("Policy evaluation completed successfully"), "Should log successful completion");
    }

    /// Test event publishing
    #[tokio::test]
    #[traced_test]
    async fn test_event_publishing() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher.clone(),
            300,
        );

        // Create test data
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::new("test-policy"),
            context: TestDataBuilder::sample_evaluation_context(),
        };

        // Execute
        let result = use_case.evaluate_policy(request).await;

        // Verify
        assert!(result.is_ok(), "Policy evaluation should succeed");

        // Check if events were published
        let publisher = container.event_publisher.as_any().downcast_ref::<MockEventPublisher>().unwrap();
        let events = publisher.get_events().await;
        assert!(!events.is_empty(), "Should have published events");
        
        // Should have evaluation event and decision event
        assert!(events.iter().any(|e| e.contains("Evaluation:")), "Should have evaluation event");
        assert!(events.iter().any(|e| e.contains("Decision:")), "Should have decision event");
        assert!(logs_contain("Policy evaluation completed successfully"), "Should log successful completion");
    }

    /// Test cache key generation consistency
    #[tokio::test]
    async fn test_cache_key_generation() {
        // Setup
        let container = PolicyEvaluationContainer::new_testing();
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage,
            container.cedar_engine,
            container.policy_cache,
            container.performance_monitor,
            container.event_publisher,
            300,
        );

        // Create identical contexts
        let context1 = TestDataBuilder::sample_evaluation_context();
        let context2 = TestDataBuilder::sample_evaluation_context();

        // Generate cache keys (this would require making calculate_cache_key public or using reflection)
        // For now, we'll just verify that the same context produces the same evaluation result
        let request1 = EvaluatePolicyRequest {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            context: context1,
        };

        let request2 = EvaluatePolicyRequest {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            context: context2,
        };

        // Execute both requests
        let result1 = use_case.evaluate_policy(request1).await;
        let result2 = use_case.evaluate_policy(request2).await;

        // Verify both succeed
        assert!(result1.is_ok(), "First evaluation should succeed");
        assert!(result2.is_ok(), "Second evaluation should succeed");

        // Results should be identical
        let response1 = result1.unwrap();
        let response2 = result2.unwrap();
        assert_eq!(response1.decision.allowed, response2.decision.allowed, "Results should be identical");
    }
}