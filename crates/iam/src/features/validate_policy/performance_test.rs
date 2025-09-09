// crates/iam/src/features/validate_policy/performance_test.rs

#[cfg(test)]
mod tests {
    use super::super::dto::*;
    use super::super::ports::*;
    use super::super::use_case::*;
    use crate::infrastructure::errors::IamError;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::time::timeout;

    // High-performance mock implementations for performance testing
    struct FastMockSyntaxValidator;

    #[async_trait]
    impl PolicySyntaxValidator for FastMockSyntaxValidator {
        async fn validate_syntax(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
            // Simulate fast syntax validation
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(vec![])
        }

        async fn is_syntax_valid(&self, _policy_content: &str) -> Result<bool, IamError> {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(true)
        }
    }

    struct FastMockSemanticValidator;

    #[async_trait]
    impl PolicySemanticValidator for FastMockSemanticValidator {
        async fn validate_semantics(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(vec![])
        }

        async fn validate_semantics_with_schema(&self, policy_content: &str, _schema_version: &str) -> Result<Vec<ValidationError>, IamError> {
            self.validate_semantics(policy_content).await
        }

        async fn is_semantically_valid(&self, _policy_content: &str) -> Result<bool, IamError> {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(true)
        }
    }

    struct FastMockHrnValidator;

    #[async_trait]
    impl PolicyHrnValidator for FastMockHrnValidator {
        async fn validate_hrns(&self, _policy_content: &str) -> Result<Vec<ValidationError>, IamError> {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(vec![])
        }

        async fn extract_and_validate_hrns(&self, _policy_content: &str) -> Result<Vec<String>, IamError> {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(vec![])
        }
    }

    struct FastMockCrossPolicyAnalyzer;

    #[async_trait]
    impl CrossPolicyAnalyzer for FastMockCrossPolicyAnalyzer {
        async fn detect_conflicts(&self, _policies: &[&str]) -> Result<Vec<PolicyConflict>, IamError> {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok(vec![])
        }

        async fn find_redundancies(&self, _policies: &[&str]) -> Result<Vec<PolicyRedundancy>, IamError> {
            tokio::time::sleep(Duration::from_millis(3)).await;
            Ok(vec![])
        }

        async fn analyze_coverage(&self, _policies: &[&str]) -> Result<CoverageAnalysis, IamError> {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(CoverageAnalysis {
                overall_coverage: 95.0,
                uncovered_entities: vec![],
                uncovered_actions: vec![],
                coverage_by_entity: std::collections::HashMap::new(),
            })
        }
    }

    struct FastMockMetricsCollector;

    #[async_trait]
    impl ValidationMetricsCollector for FastMockMetricsCollector {
        async fn start_validation_metrics(&self) -> Result<ValidationMetricsSession, IamError> {
            Ok(ValidationMetricsSession {
                session_id: "perf-test".to_string(),
                start_time: Instant::now(),
                step_metrics: std::collections::HashMap::new(),
            })
        }

        async fn record_validation_step(&self, _session: &ValidationMetricsSession, _step: ValidationStep, _duration_ms: u64) -> Result<(), IamError> {
            Ok(())
        }

        async fn finish_validation_metrics(&self, session: ValidationMetricsSession) -> Result<ValidationMetrics, IamError> {
            let elapsed = session.start_time.elapsed();
            Ok(ValidationMetrics {
                validation_time_ms: elapsed.as_millis() as u64,
                syntax_validation_time_ms: 1,
                semantic_validation_time_ms: 2,
                hrn_validation_time_ms: 1,
                memory_usage_bytes: Some(1024),
            })
        }
    }

    struct FastMockSchemaProvider;

    #[async_trait]
    impl ValidationSchemaProvider for FastMockSchemaProvider {
        async fn get_schema_info(&self) -> Result<SchemaValidationInfo, IamError> {
            Ok(SchemaValidationInfo {
                version: "v1.0".to_string(),
                entities_validated: vec!["User".to_string()],
                actions_validated: vec!["read".to_string()],
            })
        }

        async fn get_schema_for_version(&self, _version: &str) -> Result<String, IamError> {
            Ok("mock schema".to_string())
        }

        async fn validate_against_schema(&self, _policy_content: &str, _schema_version: &str) -> Result<Vec<ValidationError>, IamError> {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(vec![])
        }
    }

    struct FastMockResultAggregator;

    impl ValidationResultAggregator for FastMockResultAggregator {
        fn aggregate_validation_results(
            &self,
            syntax_errors: Vec<ValidationError>,
            semantic_errors: Vec<ValidationError>,
            hrn_errors: Vec<ValidationError>,
            warnings: Vec<ValidationWarning>,
            cross_policy_issues: Vec<PolicyConflict>,
            schema_info: Option<SchemaValidationInfo>,
        ) -> PolicyValidationResult {
            PolicyValidationResult {
                syntax_errors,
                semantic_errors,
                hrn_errors,
                warnings,
                cross_policy_issues,
                schema_info,
            }
        }

        fn determine_overall_validity(&self, result: &PolicyValidationResult) -> bool {
            result.syntax_errors.is_empty() && 
            result.semantic_errors.is_empty() && 
            result.hrn_errors.is_empty()
        }
    }

    struct FastMockEventPublisher;

    #[async_trait]
    impl ValidationEventPublisher for FastMockEventPublisher {
        async fn publish_validation_started(&self, _command: &ValidatePolicyCommand) -> Result<(), IamError> {
            Ok(())
        }

        async fn publish_validation_completed(&self, _response: &ValidatePolicyResponse) -> Result<(), IamError> {
            Ok(())
        }

        async fn publish_batch_validation_started(&self, _command: &ValidatePoliciesBatchCommand) -> Result<(), IamError> {
            Ok(())
        }

        async fn publish_batch_validation_completed(&self, _response: &ValidatePoliciesBatchResponse) -> Result<(), IamError> {
            Ok(())
        }
    }

    fn create_fast_use_case() -> ValidatePolicyUseCase {
        ValidatePolicyUseCase::new(
            Arc::new(FastMockSyntaxValidator),
            Arc::new(FastMockSemanticValidator),
            Arc::new(FastMockHrnValidator),
            Arc::new(FastMockCrossPolicyAnalyzer),
            Arc::new(FastMockMetricsCollector),
            Arc::new(FastMockSchemaProvider),
            Arc::new(FastMockResultAggregator),
            Arc::new(FastMockEventPublisher),
        )
    }

    #[tokio::test]
    async fn test_single_policy_validation_performance() {
        let use_case = create_fast_use_case();
        
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: None,
            requested_by: "perf_test".to_string(),
        };

        let start = Instant::now();
        let result = use_case.execute(command).await;
        let duration = start.elapsed();

        assert!(result.is_ok(), "Validation should succeed");
        assert!(duration < Duration::from_millis(100), "Single validation should complete in under 100ms, took: {:?}", duration);

        let response = result.unwrap();
        assert!(response.is_valid);
        assert!(response.metrics.validation_time_ms < 100);
    }

    #[tokio::test]
    async fn test_concurrent_policy_validation_performance() {
        let use_case = Arc::new(create_fast_use_case());
        
        let num_concurrent = 100;
        let mut handles = Vec::new();

        let start = Instant::now();

        for i in 0..num_concurrent {
            let use_case_clone = use_case.clone();
            let handle = tokio::spawn(async move {
                let command = ValidatePolicyCommand {
                    content: format!("permit(principal, action, resource{});", i),
                    options: None,
                    requested_by: "perf_test".to_string(),
                };
                use_case_clone.execute(command).await
            });
            handles.push(handle);
        }

        let results: Vec<_> = futures::future::join_all(handles).await;
        let duration = start.elapsed();

        // All validations should succeed
        for result in results {
            let validation_result = result.unwrap();
            assert!(validation_result.is_ok());
            assert!(validation_result.unwrap().is_valid);
        }

        // Should complete all 100 concurrent validations in reasonable time
        assert!(duration < Duration::from_secs(5), "100 concurrent validations should complete in under 5 seconds, took: {:?}", duration);
        
        let avg_time_per_validation = duration.as_millis() / num_concurrent as u128;
        println!("Average time per concurrent validation: {}ms", avg_time_per_validation);
        assert!(avg_time_per_validation < 50, "Average time per validation should be under 50ms");
    }

    #[tokio::test]
    async fn test_batch_validation_performance() {
        let use_case = create_fast_use_case();
        
        // Create a batch of 100 policies
        let mut policies = Vec::new();
        for i in 0..100 {
            policies.push(PolicyToValidate {
                id: Some(format!("policy-{}", i)),
                content: format!("permit(principal, action, resource{});", i),
                metadata: None,
            });
        }

        let batch_command = ValidatePoliciesBatchCommand {
            policies,
            options: Some(ValidationOptions {
                include_warnings: Some(false), // Disable warnings for performance
                deep_validation: Some(false),  // Disable deep validation for performance
                schema_version: None,
                timeout_ms: Some(10000),
            }),
            requested_by: "perf_test".to_string(),
        };

        let start = Instant::now();
        let result = use_case.execute_batch(batch_command).await;
        let duration = start.elapsed();

        assert!(result.is_ok(), "Batch validation should succeed");
        assert!(duration < Duration::from_secs(10), "Batch of 100 policies should complete in under 10 seconds, took: {:?}", duration);

        let response = result.unwrap();
        assert!(response.overall_valid);
        assert_eq!(response.individual_results.len(), 100);
        assert_eq!(response.batch_metrics.valid_policies, 100);
        
        let avg_time_per_policy = duration.as_millis() / 100;
        println!("Average time per policy in batch: {}ms", avg_time_per_policy);
        assert!(avg_time_per_policy < 100, "Average time per policy in batch should be under 100ms");
    }

    #[tokio::test]
    async fn test_large_policy_validation_performance() {
        let use_case = create_fast_use_case();
        
        // Create a large policy (10KB)
        let large_policy_content = format!(
            "permit(principal, action, resource) when {{\n{}\n}};",
            (0..1000).map(|i| format!("    context.attr{} == \"value{}\"", i, i)).collect::<Vec<_>>().join(" &&\n")
        );

        let command = ValidatePolicyCommand {
            content: large_policy_content,
            options: None,
            requested_by: "perf_test".to_string(),
        };

        let start = Instant::now();
        let result = use_case.execute(command).await;
        let duration = start.elapsed();

        assert!(result.is_ok(), "Large policy validation should succeed");
        assert!(duration < Duration::from_secs(1), "Large policy validation should complete in under 1 second, took: {:?}", duration);

        let response = result.unwrap();
        assert!(response.is_valid);
        println!("Large policy validation time: {:?}", duration);
    }

    #[tokio::test]
    async fn test_validation_timeout_performance() {
        let use_case = create_fast_use_case();
        
        let command = ValidatePolicyCommand {
            content: "permit(principal, action, resource);".to_string(),
            options: Some(ValidationOptions {
                include_warnings: Some(true),
                deep_validation: Some(true),
                schema_version: None,
                timeout_ms: Some(50), // Very short timeout
            }),
            requested_by: "perf_test".to_string(),
        };

        let start = Instant::now();
        let result = timeout(Duration::from_millis(100), use_case.execute(command)).await;
        let duration = start.elapsed();

        match result {
            Ok(validation_result) => {
                // If validation completed within timeout
                assert!(validation_result.is_ok());
                assert!(duration < Duration::from_millis(100));
            }
            Err(_) => {
                // If validation timed out
                assert!(duration >= Duration::from_millis(50));
                assert!(duration < Duration::from_millis(150)); // Should not take much longer than timeout
            }
        }
    }

    #[tokio::test]
    async fn test_memory_usage_performance() {
        let use_case = create_fast_use_case();
        
        // Test with various policy sizes to check memory usage
        let policy_sizes = vec![100, 1000, 10000]; // bytes
        
        for size in policy_sizes {
            let policy_content = "permit(principal, action, resource);".repeat(size / 40); // Approximate size
            
            let command = ValidatePolicyCommand {
                content: policy_content,
                options: None,
                requested_by: "perf_test".to_string(),
            };

            let result = use_case.execute(command).await;
            assert!(result.is_ok(), "Validation should succeed for policy size: {}", size);

            let response = result.unwrap();
            if let Some(memory_usage) = response.metrics.memory_usage_bytes {
                println!("Memory usage for policy size {}: {} bytes", size, memory_usage);
                // Memory usage should be reasonable (not more than 10x the policy size)
                assert!(memory_usage < (size * 10) as u64, "Memory usage should be reasonable");
            }
        }
    }

    #[tokio::test]
    async fn test_throughput_performance() {
        let use_case = Arc::new(create_fast_use_case());
        
        let duration = Duration::from_secs(5);
        let start = Instant::now();
        let mut completed_validations = 0;
        let mut handles = Vec::new();

        // Spawn continuous validation tasks
        while start.elapsed() < duration {
            let use_case_clone = use_case.clone();
            let handle = tokio::spawn(async move {
                let command = ValidatePolicyCommand {
                    content: "permit(principal, action, resource);".to_string(),
                    options: None,
                    requested_by: "perf_test".to_string(),
                };
                use_case_clone.execute(command).await
            });
            handles.push(handle);
            
            // Limit concurrent tasks to avoid overwhelming the system
            if handles.len() >= 50 {
                let results = futures::future::join_all(handles).await;
                completed_validations += results.len();
                handles = Vec::new();
            }
        }

        // Wait for remaining tasks
        if !handles.is_empty() {
            let results = futures::future::join_all(handles).await;
            completed_validations += results.len();
        }

        let actual_duration = start.elapsed();
        let throughput = completed_validations as f64 / actual_duration.as_secs_f64();
        
        println!("Completed {} validations in {:?}", completed_validations, actual_duration);
        println!("Throughput: {:.2} validations/second", throughput);
        
        // Should achieve reasonable throughput (at least 100 validations/second with fast mocks)
        assert!(throughput > 100.0, "Throughput should be at least 100 validations/second, got: {:.2}", throughput);
    }

    #[tokio::test]
    async fn test_stress_test_validation() {
        let use_case = create_fast_use_case();
        
        // Stress test with a very large batch
        let mut policies = Vec::new();
        for i in 0..1000 {
            policies.push(PolicyToValidate {
                id: Some(format!("stress-policy-{}", i)),
                content: format!("permit(principal == User::\"user{}\", action, resource);", i),
                metadata: None,
            });
        }

        let batch_command = ValidatePoliciesBatchCommand {
            policies,
            options: Some(ValidationOptions {
                include_warnings: Some(false),
                deep_validation: Some(false),
                schema_version: None,
                timeout_ms: Some(30000), // 30 second timeout
            }),
            requested_by: "stress_test".to_string(),
        };

        let start = Instant::now();
        let result = use_case.execute_batch(batch_command).await;
        let duration = start.elapsed();

        assert!(result.is_ok(), "Stress test should succeed");
        assert!(duration < Duration::from_secs(30), "Stress test should complete within timeout, took: {:?}", duration);

        let response = result.unwrap();
        assert_eq!(response.individual_results.len(), 1000);
        assert!(response.overall_valid);
        
        println!("Stress test completed 1000 policies in {:?}", duration);
        println!("Average time per policy: {:?}", duration / 1000);
    }

    #[tokio::test]
    async fn test_validation_latency_percentiles() {
        let use_case = Arc::new(create_fast_use_case());
        let num_samples = 1000;
        let mut latencies = Vec::new();

        // Collect latency samples
        for i in 0..num_samples {
            let use_case_clone = use_case.clone();
            let start = Instant::now();
            
            let command = ValidatePolicyCommand {
                content: format!("permit(principal, action, resource{});", i),
                options: None,
                requested_by: "latency_test".to_string(),
            };

            let result = use_case_clone.execute(command).await;
            let latency = start.elapsed();
            
            assert!(result.is_ok());
            latencies.push(latency.as_millis() as u64);
        }

        // Sort latencies for percentile calculation
        latencies.sort();

        let p50 = latencies[num_samples * 50 / 100];
        let p95 = latencies[num_samples * 95 / 100];
        let p99 = latencies[num_samples * 99 / 100];
        let max = latencies[num_samples - 1];

        println!("Latency percentiles:");
        println!("P50: {}ms", p50);
        println!("P95: {}ms", p95);
        println!("P99: {}ms", p99);
        println!("Max: {}ms", max);

        // Performance assertions
        assert!(p50 < 50, "P50 latency should be under 50ms, got: {}ms", p50);
        assert!(p95 < 100, "P95 latency should be under 100ms, got: {}ms", p95);
        assert!(p99 < 200, "P99 latency should be under 200ms, got: {}ms", p99);
        assert!(max < 500, "Max latency should be under 500ms, got: {}ms", max);
    }
}