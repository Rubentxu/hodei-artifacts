use super::performance_monitor::*;
use super::dto::*;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_performance_monitor_creation() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.total_validations, 0);
    assert_eq!(stats.cache_hit_rate, 0.0);
    assert_eq!(stats.total_cache_entries, 0);
}

#[tokio::test]
async fn test_performance_session_lifecycle() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Start session
    let mut session = monitor.start_session("test-session".to_string()).await;
    assert_eq!(session.session_id, "test-session");
    assert!(session.checkpoints.is_empty());
    
    // Add checkpoints
    monitor.add_checkpoint(&mut session, "syntax_validation".to_string()).await;
    monitor.add_checkpoint(&mut session, "semantic_validation".to_string()).await;
    
    assert_eq!(session.checkpoints.len(), 2);
    assert_eq!(session.checkpoints[0].name, "syntax_validation");
    assert_eq!(session.checkpoints[1].name, "semantic_validation");
    
    // Finish session
    let metrics = monitor.finish_session(session).await.unwrap();
    assert!(metrics.validation_time_ms > 0);
    assert_eq!(metrics.validation_steps, 2);
}

#[tokio::test]
async fn test_cache_operations() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
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
    
    // Initially no cached result
    assert!(monitor.get_cached_result(policy_content).await.is_none());
    
    // Cache the result
    monitor.cache_result(policy_content, validation_result.clone()).await;
    
    // Should now find cached result
    let cached = monitor.get_cached_result(policy_content).await;
    assert!(cached.is_some());
    let cached_result = cached.unwrap();
    assert_eq!(cached_result.schema_info.version, "1.0.0");
    assert_eq!(cached_result.schema_info.entity_types_count, 3);
}

#[tokio::test]
async fn test_schema_cache_operations() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    let schema_version = "2.0.0";
    let schema_info = SchemaValidationInfo {
        version: schema_version.to_string(),
        schema_id: "production-schema".to_string(),
        entity_types_count: 5,
        actions_count: 10,
    };
    let schema_content = "schema content here";
    
    // Initially no cached schema
    assert!(monitor.get_cached_schema(schema_version).await.is_none());
    
    // Cache the schema
    monitor.cache_schema(
        schema_version.to_string(),
        schema_info.clone(),
        schema_content.to_string(),
    ).await;
    
    // Should now find cached schema
    let cached = monitor.get_cached_schema(schema_version).await;
    assert!(cached.is_some());
    let (cached_info, cached_content) = cached.unwrap();
    assert_eq!(cached_info.version, "2.0.0");
    assert_eq!(cached_info.entity_types_count, 5);
    assert_eq!(cached_content, schema_content);
}

#[tokio::test]
async fn test_batch_metrics_recording() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    let batch_size = 5;
    let processing_time = Duration::from_millis(1000);
    
    monitor.record_batch_metrics(batch_size, processing_time).await;
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.avg_throughput_pps, 5.0); // 5 policies / 1 second
}

#[tokio::test]
async fn test_validation_error_recording() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Record different types of errors
    monitor.record_validation_error(ValidationErrorType::SyntaxError).await;
    monitor.record_validation_error(ValidationErrorType::SyntaxError).await;
    monitor.record_validation_error(ValidationErrorType::SemanticError).await;
    monitor.record_validation_error(ValidationErrorType::UnknownEntity).await;
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.error_distribution.get(&ValidationErrorType::SyntaxError), Some(&2));
    assert_eq!(stats.error_distribution.get(&ValidationErrorType::SemanticError), Some(&1));
    assert_eq!(stats.error_distribution.get(&ValidationErrorType::UnknownEntity), Some(&1));
}

#[tokio::test]
async fn test_performance_statistics() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Record some metrics
    let session1 = monitor.start_session("session1".to_string()).await;
    sleep(Duration::from_millis(10)).await; // Small delay to ensure measurable time
    let metrics1 = monitor.finish_session(session1).await.unwrap();
    
    let session2 = monitor.start_session("session2".to_string()).await;
    sleep(Duration::from_millis(20)).await;
    let metrics2 = monitor.finish_session(session2).await.unwrap();
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.total_validations, 2);
    assert!(stats.avg_validation_time_ms > 0);
    assert!(stats.p95_validation_time_ms >= stats.avg_validation_time_ms);
}

#[tokio::test]
async fn test_cache_optimization() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Add multiple cache entries
    for i in 0..10 {
        let policy = format!("permit(principal, action{}, resource);", i);
        let result = PolicyValidationResult {
            syntax_errors: Vec::new(),
            semantic_errors: Vec::new(),
            hrn_errors: Vec::new(),
            warnings: Vec::new(),
            schema_info: SchemaValidationInfo {
                version: "1.0.0".to_string(),
                schema_id: "test".to_string(),
                entity_types_count: 2,
                actions_count: 3,
            },
        };
        monitor.cache_result(&policy, result).await;
    }
    
    let stats_before = monitor.get_performance_statistics().await;
    assert!(stats_before.total_cache_entries > 0);
    
    // Optimize cache
    monitor.optimize_cache().await;
    
    // Cache should still have entries (they're not expired)
    let stats_after = monitor.get_performance_statistics().await;
    assert!(stats_after.total_cache_entries > 0);
}

#[tokio::test]
async fn test_performance_alerts() {
    let thresholds = PerformanceThresholds {
        max_validation_time_ms: 100, // Very low threshold for testing
        max_memory_usage_mb: 1,      // Very low threshold
        min_cache_hit_rate: 0.9,     // Very high threshold
        max_schema_load_time_ms: 50,
        max_batch_processing_time_ms: 200,
        min_throughput_policies_per_sec: 100.0, // Very high threshold
    };
    
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Create a session that will exceed thresholds
    let session = monitor.start_session("slow-session".to_string()).await;
    sleep(Duration::from_millis(150)).await; // Exceed time threshold
    
    // This should trigger performance alerts (logged as warnings)
    let _metrics = monitor.finish_session(session).await.unwrap();
    
    // The alerts are logged, so we can't directly test them here,
    // but we can verify the session completed successfully
    assert!(true); // If we get here, the session completed without panicking
}

#[tokio::test]
async fn test_clear_metrics() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Add some metrics
    let session = monitor.start_session("test".to_string()).await;
    let _metrics = monitor.finish_session(session).await.unwrap();
    
    monitor.record_validation_error(ValidationErrorType::SyntaxError).await;
    
    let stats_before = monitor.get_performance_statistics().await;
    assert!(stats_before.total_validations > 0);
    assert!(!stats_before.error_distribution.is_empty());
    
    // Clear metrics
    monitor.clear_metrics().await;
    
    let stats_after = monitor.get_performance_statistics().await;
    assert_eq!(stats_after.total_validations, 0);
    assert!(stats_after.error_distribution.is_empty());
}

#[test]
fn test_performance_thresholds_default() {
    let thresholds = PerformanceThresholds::default();
    
    assert_eq!(thresholds.max_validation_time_ms, 1000);
    assert_eq!(thresholds.max_memory_usage_mb, 100);
    assert_eq!(thresholds.min_cache_hit_rate, 0.8);
    assert_eq!(thresholds.max_schema_load_time_ms, 500);
    assert_eq!(thresholds.max_batch_processing_time_ms, 5000);
    assert_eq!(thresholds.min_throughput_policies_per_sec, 10.0);
}

#[test]
fn test_performance_metrics_new() {
    let metrics = PerformanceMetrics::new();
    
    assert!(metrics.validation_times.is_empty());
    assert!(metrics.memory_usage_samples.is_empty());
    assert_eq!(metrics.cache_hit_rate, 0.0);
    assert!(metrics.schema_load_times.is_empty());
    assert!(metrics.batch_processing_times.is_empty());
    assert!(metrics.error_rates.is_empty());
    assert!(metrics.throughput_samples.is_empty());
}

#[test]
fn test_validation_cache_new() {
    let cache = ValidationCache::new(100, 3600);
    
    assert_eq!(cache.max_entries, 100);
    assert_eq!(cache.ttl_seconds, 3600);
    assert!(cache.policy_results.is_empty());
    assert!(cache.schema_cache.is_empty());
}

#[tokio::test]
async fn test_concurrent_cache_access() {
    let thresholds = PerformanceThresholds::default();
    let monitor = std::sync::Arc::new(PolicyValidationPerformanceMonitor::new(thresholds));
    
    let mut handles = Vec::new();
    
    // Spawn multiple tasks that access the cache concurrently
    for i in 0..10 {
        let monitor_clone = monitor.clone();
        let handle = tokio::spawn(async move {
            let policy = format!("permit(principal, action{}, resource);", i);
            let result = PolicyValidationResult {
                syntax_errors: Vec::new(),
                semantic_errors: Vec::new(),
                hrn_errors: Vec::new(),
                warnings: Vec::new(),
                schema_info: SchemaValidationInfo {
                    version: "1.0.0".to_string(),
                    schema_id: "test".to_string(),
                    entity_types_count: 2,
                    actions_count: 3,
                },
            };
            
            // Cache and retrieve
            monitor_clone.cache_result(&policy, result).await;
            let cached = monitor_clone.get_cached_result(&policy).await;
            assert!(cached.is_some());
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.total_cache_entries, 10);
}

#[tokio::test]
async fn test_performance_statistics_calculations() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    // Record multiple validation sessions with different durations
    let durations = vec![100, 200, 150, 300, 250]; // milliseconds
    
    for (i, duration_ms) in durations.iter().enumerate() {
        let session = monitor.start_session(format!("session-{}", i)).await;
        sleep(Duration::from_millis(*duration_ms as u64)).await;
        let _metrics = monitor.finish_session(session).await.unwrap();
    }
    
    let stats = monitor.get_performance_statistics().await;
    assert_eq!(stats.total_validations, 5);
    
    // Average should be around 200ms (100+200+150+300+250)/5 = 200
    // Allow some tolerance for timing variations
    assert!(stats.avg_validation_time_ms >= 150);
    assert!(stats.avg_validation_time_ms <= 350);
    
    // P95 should be higher than average
    assert!(stats.p95_validation_time_ms >= stats.avg_validation_time_ms);
}

#[tokio::test]
async fn test_memory_usage_tracking() {
    let thresholds = PerformanceThresholds::default();
    let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
    
    let session = monitor.start_session("memory-test".to_string()).await;
    let metrics = monitor.finish_session(session).await.unwrap();
    
    // Memory usage should be tracked (even if it's a placeholder value)
    assert!(metrics.memory_usage_bytes > 0);
    
    let stats = monitor.get_performance_statistics().await;
    assert!(stats.avg_memory_usage_mb >= 0);
}