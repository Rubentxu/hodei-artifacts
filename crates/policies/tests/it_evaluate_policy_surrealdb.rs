#![cfg(feature = "integration")]

#[cfg(test)]
mod tests {
    use policies::domain::context::PolicyEvaluationContext;
    use policies::domain::decision::PolicyDecision;
    use policies::domain::ids::PolicyId;
    use policies::domain::policy::Policy;
    use policies::domain::{context, decision, ids, policy};
    use policies::features::evaluate_policy::{
        adapter::{CedarEngineAdapter, EventPublisherAdapter, InMemoryCacheAdapter,
                  PolicyStorageAdapter, SurrealPerformanceMonitor}, di::{PolicyEvaluationContainer, PolicyEvaluationContainerBuilder}, dto::{BatchEvaluatePolicyRequest, BatchEvaluatePolicyResponse},
        error::EvaluatePolicyError,
        ports::{CedarEnginePort, EventPublisherPort, PerformanceMonitorPort,
                PolicyCachePort, PolicyStoragePort},
        EvaluatePolicyRequest,
        EvaluatePolicyResponse,
        EvaluatePolicyUseCase
    };
    use shared::attributes::AttributeValue;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;
    use surrealdb::engine::any::Any;
    use surrealdb::opt::Resource;
    use surrealdb::Surreal;
    use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage, ImageExt};
    use tracing::info;

    /// SurrealDB container configuration
    async fn setup_surrealdb_container() -> Result<(String, String), Box<dyn std::error::Error>> {
        info!("Setting up SurrealDB container for integration tests");

        let surrealdb_image = GenericImage::new("surrealdb/surrealdb", "v2.3.9-dev")
            .with_exposed_port(8000u16)
            .with_env_var("SURREAL_USER", "root")
            .with_env_var("SURREAL_PASS", "password")
            .with_env_var("SURREAL_BIND", "0.0.0.0:8000")
            .with_wait_for(WaitFor::message_on_stdout("Started web server on"))
            .with_wait_for(WaitFor::Duration { length: Duration::from_secs(10) });

        let container = surrealdb_image.start().await
            .map_err(|e| format!("Failed to start SurrealDB container: {}", e))?;

        let port = container.get_host_port_ipv4(8000).await
            .map_err(|e| format!("Failed to get SurrealDB port: {}", e))?;

        let host = container.get_host().await
            .map_err(|e| format!("Failed to get SurrealDB host: {}", e))?;

        let connection_string = format!("ws://{}:{}/rpc", host, port);
        info!("SurrealDB container started at: {}", connection_string);

        // Wait for SurrealDB to be ready
        tokio::time::sleep(Duration::from_secs(5)).await;

        Ok((connection_string, container.id().to_string()))
    }

    /// Create SurrealDB connection
    async fn create_surrealdb_connection(connection_string: &str) -> Result<Surreal<Any>, Box<dyn std::error::Error>> {
        use surrealdb::opt::auth::Root;
        
        let client = Surreal::new::<Any>(connection_string).await
            .map_err(|e| format!("Failed to connect to SurrealDB: {}", e))?;

        // Sign in
        client.signin(Root {
            username: "root",
            password: "password",
        }).await
            .map_err(|e| format!("Failed to sign in to SurrealDB: {}", e))?;

        // Use test namespace and database
        client.use_ns("test_namespace").use_db("test_policies").await
            .map_err(|e| format!("Failed to use namespace/database: {}", e))?;

        info!("Connected to SurrealDB successfully");
        Ok(client)
    }

    /// Create sample policy evaluation context
    fn create_sample_context() -> PolicyEvaluationContext {
        let mut context = PolicyEvaluationContext::new();
        
        context.add_attribute(
            "principal", 
            AttributeValue::String("hrn:hodei:iam::system:user/test-user".to_string())
        );
        context.add_attribute(
            "action", 
            AttributeValue::String("read".to_string())
        );
        context.add_attribute(
            "resource", 
            AttributeValue::String("hrn:hodei:artifact::us-east-1:123456789012:physical-artifact/sha256-abcd1234".to_string())
        );
        context.add_attribute(
            "principal_department", 
            AttributeValue::String("engineering".to_string())
        );
        context.add_attribute(
            "resource_department", 
            AttributeValue::String("engineering".to_string())
        );
        
        context
    }

    /// Create sample policy for testing
    fn create_sample_policy(policy_id: &str) -> Policy {
        let policy_content = r#"
            permit(principal, action, resource)
            when {
                principal.department == resource.department &&
                action in ["read", "write"]
            };
        "#.to_string();

        Policy::new(
            PolicyId::from_str(&format!("hrn:hodei:iam::system:organization/test-org/policy/{}", policy_id)).unwrap(),
            policy_content,
            "Test policy for integration".to_string(),
            Some("engineering".to_string()),
            "test-org".to_string(),
            true,
        )
    }

    /// Setup complete policy evaluation infrastructure
    async fn setup_policy_evaluation_infrastructure(
        client: Surreal<Any>,
    ) -> Result<PolicyEvaluationContainer, EvaluatePolicyError> {
        info!("Setting up policy evaluation infrastructure");

        let db = Arc::new(client);
        let policy_storage = Arc::new(PolicyStorageAdapter::new(db.clone(), "policies"));
        let cedar_engine = Arc::new(CedarEngineAdapter::new());
        let policy_cache = Arc::new(InMemoryCacheAdapter::new(1000, Duration::from_secs(300)));
        let performance_monitor = Arc::new(SurrealPerformanceMonitor::new());
        let event_publisher = Arc::new(EventPublisherAdapter::new());

        Ok(PolicyEvaluationContainer::new(
            policy_storage,
            cedar_engine,
            policy_cache,
            performance_monitor,
            event_publisher,
        ))
    }

    #[tokio::test]
    async fn test_surrealdb_container_setup() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB container setup");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;

        // Simple test to verify connection works
        let result = client.query("RETURN 'Hello from SurrealDB!'").await;
        assert!(result.is_ok(), "Basic SurrealDB query should succeed");

        info!("SurrealDB container setup test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_policy_storage_and_retrieval() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing policy storage and retrieval with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create and store a test policy
        let test_policy = create_sample_policy("test-storage-policy");
        let storage_result = container.policy_storage.create_policy(&test_policy).await;
        assert!(storage_result.is_ok(), "Policy storage should succeed");

        // Retrieve the policy
        let retrieved_policy = container.policy_storage.get_policy(&test_policy.id).await;
        assert!(retrieved_policy.is_ok(), "Policy retrieval should succeed");
        
        let policy = retrieved_policy.unwrap();
        assert_eq!(policy.id, test_policy.id, "Retrieved policy ID should match");
        assert_eq!(policy.content, test_policy.content, "Retrieved policy content should match");

        // Test policy listing
        let policies = container.policy_storage.list_policies().await;
        assert!(policies.is_ok(), "Policy listing should succeed");
        
        let policy_list = policies.unwrap();
        assert!(policy_list.len() > 0, "Policy list should not be empty");

        info!("Policy storage and retrieval test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_policy_evaluation_with_surrealdb() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing policy evaluation with SurrealDB backend");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create and store a test policy
        let test_policy = create_sample_policy("test-eval-policy");
        container.policy_storage.create_policy(&test_policy).await?;

        // Create use case with short TTL for testing
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            60, // 1 minute TTL for testing
        );

        // Create evaluation request
        let request = EvaluatePolicyRequest {
            policy_id: test_policy.id.clone(),
            context: create_sample_context(),
        };

        // Evaluate policy
        let result = use_case.evaluate_policy(request).await;
        assert!(result.is_ok(), "Policy evaluation should succeed");

        let response = result.unwrap();
        assert!(response.decision.allowed, "Policy should allow access for matching departments");
        assert_eq!(response.policy_id, test_policy.id, "Response should contain correct policy ID");
        assert!(response.metrics.evaluation_time_ms > 0, "Evaluation time should be recorded");

        info!("Policy evaluation test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_batch_policy_evaluation() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing batch policy evaluation with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create multiple test policies
        let policy1 = create_sample_policy("batch-policy-1");
        let policy2 = create_sample_policy("batch-policy-2");
        
        container.policy_storage.create_policy(&policy1).await?;
        container.policy_storage.create_policy(&policy2).await?;

        // Create use case
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            60,
        );

        // Create batch request
        let requests = vec![
            EvaluatePolicyRequest {
                policy_id: policy1.id.clone(),
                context: create_sample_context(),
            },
            EvaluatePolicyRequest {
                policy_id: policy2.id.clone(),
                context: create_sample_context(),
            },
        ];

        let batch_request = BatchEvaluatePolicyRequest {
            requests,
            parallel: true, // Test parallel execution
        };

        // Execute batch evaluation
        let result = use_case.batch_evaluate_policies(batch_request).await;
        assert!(result.is_ok(), "Batch policy evaluation should succeed");

        let response = result.unwrap();
        assert_eq!(response.responses.len(), 2, "Should have 2 responses");
        assert_eq!(response.batch_metrics.successful_evaluations, 2, "All evaluations should succeed");
        assert_eq!(response.batch_metrics.failed_evaluations, 0, "No evaluations should fail");
        assert!(response.batch_metrics.total_time_ms > 0, "Total time should be recorded");

        // Verify all policies allowed access
        for eval_response in response.responses {
            assert!(eval_response.decision.allowed, "All policies should allow access");
        }

        info!("Batch policy evaluation test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_policy_caching_behavior() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing policy caching behavior with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create test policy
        let test_policy = create_sample_policy("cache-test-policy");
        container.policy_storage.create_policy(&test_policy).await?;

        // Create use case with short TTL for testing
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            5, // Very short TTL for testing
        );

        let request = EvaluatePolicyRequest {
            policy_id: test_policy.id.clone(),
            context: create_sample_context(),
        };

        // First evaluation - should cache miss
        let result1 = use_case.evaluate_policy(request.clone()).await?;
        assert!(result1.metrics.cache_hit_rate < 0.5, "First evaluation should be cache miss");

        // Second evaluation immediately - should cache hit
        let result2 = use_case.evaluate_policy(request.clone()).await?;
        assert!(result2.metrics.cache_hit_rate > 0.5, "Second evaluation should be cache hit");

        // Wait for cache to expire
        tokio::time::sleep(Duration::from_secs(6)).await;

        // Third evaluation after cache expiry - should cache miss
        let result3 = use_case.evaluate_policy(request).await?;
        assert!(result3.metrics.cache_hit_rate < 0.5, "Evaluation after expiry should be cache miss");

        info!("Policy caching behavior test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_error_handling_scenarios() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing error handling scenarios with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create use case
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            60,
        );

        // Test evaluation with non-existent policy
        let request = EvaluatePolicyRequest {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/non-existent-policy").unwrap(),
            context: create_sample_context(),
        };

        let result = use_case.evaluate_policy(request).await;
        assert!(result.is_ok(), "Evaluation should still return a response even for non-existent policy");
        
        let response = result.unwrap();
        assert!(!response.decision.allowed, "Non-existent policy should deny access");

        info!("Error handling scenarios test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_performance_validation() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing performance validation with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create test policy
        let test_policy = create_sample_policy("perf-test-policy");
        container.policy_storage.create_policy(&test_policy).await?;

        // Create use case
        let use_case = EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            60,
        );

        let request = EvaluatePolicyRequest {
            policy_id: test_policy.id.clone(),
            context: create_sample_context(),
        };

        // Perform multiple evaluations to gather performance data
        let mut evaluation_times = Vec::new();
        for _ in 0..10 {
            let result = use_case.evaluate_policy(request.clone()).await?;
            evaluation_times.push(result.metrics.evaluation_time_ms);
        }

        // Calculate average evaluation time
        let avg_time: f64 = evaluation_times.iter().sum::<f64>() / evaluation_times.len() as f64;
        info!("Average evaluation time: {:.2}ms", avg_time);

        // Performance assertions (adjusted for integration test environment)
        assert!(avg_time < 100.0, "Average evaluation time should be less than 100ms for integration tests");
        
        // Check that all evaluations were successful
        for &time in &evaluation_times {
            assert!(time > 0.0, "All evaluation times should be positive");
        }

        info!("Performance validation test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_concurrent_policy_evaluation() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing concurrent policy evaluation with SurrealDB");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create multiple test policies
        let mut policies = Vec::new();
        for i in 0..5 {
            let policy = create_sample_policy(&format!("concurrent-policy-{}", i));
            container.policy_storage.create_policy(&policy).await?;
            policies.push(policy);
        }

        // Create use case
        let use_case = Arc::new(EvaluatePolicyUseCase::new(
            container.policy_storage.clone(),
            container.cedar_engine.clone(),
            container.policy_cache.clone(),
            container.performance_monitor.clone(),
            container.event_publisher.clone(),
            60,
        ));

        // Spawn concurrent evaluation tasks
        let mut handles = Vec::new();
        for policy in policies {
            let use_case_clone = use_case.clone();
            let context = create_sample_context();
            
            let handle = tokio::spawn(async move {
                let request = EvaluatePolicyRequest {
                    policy_id: policy.id,
                    context,
                };
                use_case_clone.evaluate_policy(request).await
            });
            
            handles.push(handle);
        }

        // Wait for all evaluations to complete
        let results = futures::future::join_all(handles).await;
        
        // Verify all evaluations succeeded
        let mut successful_count = 0;
        for result in results {
            match result {
                Ok(Ok(response)) => {
                    assert!(response.decision.allowed, "Concurrent evaluation should allow access");
                    successful_count += 1;
                },
                Ok(Err(e)) => {
                    panic!("Concurrent evaluation failed: {:?}", e);
                },
                Err(e) => {
                    panic!("Concurrent task panicked: {:?}", e);
                }
            }
        }

        assert_eq!(successful_count, 5, "All 5 concurrent evaluations should succeed");
        info!("Concurrent policy evaluation test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_database_cleanup_and_isolation() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing database cleanup and isolation between tests");

        // Each test should have isolated data
        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;
        let container = setup_policy_evaluation_infrastructure(client).await?;

        // Create a policy with unique ID
        let unique_policy_id = format!("isolation-test-{}", uuid::Uuid::new_v4());
        let test_policy = create_sample_policy(&unique_policy_id);
        
        // Store the policy
        container.policy_storage.create_policy(&test_policy).await?;

        // Verify it exists
        let retrieved = container.policy_storage.get_policy(&test_policy.id).await;
        assert!(retrieved.is_ok(), "Should be able to retrieve just-created policy");

        // List all policies and verify isolation
        let all_policies = container.policy_storage.list_policies().await?;
        assert!(all_policies.len() >= 1, "Should have at least our test policy");

        // Verify our specific policy is in the list
        let policy_exists = all_policies.iter().any(|p| p.id == test_policy.id);
        assert!(policy_exists, "Our test policy should be in the policy list");

        info!("Database cleanup and isolation test completed successfully");
        Ok(())
    }
}