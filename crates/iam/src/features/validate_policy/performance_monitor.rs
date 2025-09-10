use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use crate::features::validate_policy::dto::*;
use crate::infrastructure::errors::IamError;

/// Performance monitoring and optimization for policy validation
pub struct PolicyValidationPerformanceMonitor {
    metrics: Arc<RwLock<PerformanceMetrics>>,
    thresholds: PerformanceThresholds,
    cache: Arc<RwLock<ValidationCache>>,
}

/// Performance metrics collection
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub validation_times: Vec<Duration>,
    pub memory_usage_samples: Vec<usize>,
    pub cache_hit_rate: f64,
    pub schema_load_times: Vec<Duration>,
    pub batch_processing_times: Vec<Duration>,
    pub error_rates: HashMap<ValidationErrorType, usize>,
    pub throughput_samples: Vec<f64>, // policies per second
}

/// Performance thresholds for monitoring
#[derive(Debug, Clone)]
pub struct PerformanceThresholds {
    pub max_validation_time_ms: u64,
    pub max_memory_usage_mb: usize,
    pub min_cache_hit_rate: f64,
    pub max_schema_load_time_ms: u64,
    pub max_batch_processing_time_ms: u64,
    pub min_throughput_policies_per_sec: f64,
}

/// Validation cache for performance optimization
#[derive(Debug)]
pub struct ValidationCache {
    policy_results: HashMap<String, CachedValidationResult>,
    schema_cache: HashMap<String, CachedSchema>,
    max_entries: usize,
    ttl_seconds: u64,
}

/// Cached validation result
#[derive(Debug, Clone)]
pub struct CachedValidationResult {
    result: PolicyValidationResult,
    cached_at: Instant,
    access_count: usize,
}

/// Cached schema information
#[derive(Debug, Clone)]
pub struct CachedSchema {
    schema_info: SchemaValidationInfo,
    schema_content: String,
    cached_at: Instant,
    access_count: usize,
}

/// Performance monitoring session
#[derive(Debug)]
pub struct PerformanceSession {
    session_id: String,
    started_at: Instant,
    checkpoints: Vec<PerformanceCheckpoint>,
    memory_baseline: usize,
}

/// Performance checkpoint for detailed monitoring
#[derive(Debug, Clone)]
pub struct PerformanceCheckpoint {
    name: String,
    timestamp: Instant,
    memory_usage: usize,
    duration_from_start: Duration,
}

/// Performance alert types
#[derive(Debug, Clone)]
pub enum PerformanceAlert {
    ValidationTimeExceeded { actual_ms: u64, threshold_ms: u64 },
    MemoryUsageExceeded { actual_mb: usize, threshold_mb: usize },
    CacheHitRateLow { actual_rate: f64, threshold_rate: f64 },
    SchemaLoadTimeSlow { actual_ms: u64, threshold_ms: u64 },
    ThroughputLow { actual_pps: f64, threshold_pps: f64 },
}

impl PolicyValidationPerformanceMonitor {
    pub fn new(thresholds: PerformanceThresholds) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(PerformanceMetrics::new())),
            thresholds,
            cache: Arc::new(RwLock::new(ValidationCache::new(1000, 3600))), // 1000 entries, 1 hour TTL
        }
    }

    /// Start a performance monitoring session
    pub async fn start_session(&self, session_id: String) -> PerformanceSession {
        PerformanceSession {
            session_id,
            started_at: Instant::now(),
            checkpoints: Vec::new(),
            memory_baseline: self.get_current_memory_usage(),
        }
    }

    /// Add a checkpoint to the performance session
    pub async fn add_checkpoint(&self, session: &mut PerformanceSession, name: String) {
        let checkpoint = PerformanceCheckpoint {
            name,
            timestamp: Instant::now(),
            memory_usage: self.get_current_memory_usage(),
            duration_from_start: session.started_at.elapsed(),
        };
        session.checkpoints.push(checkpoint);
    }

    /// Finish performance session and record metrics
    pub async fn finish_session(&self, session: PerformanceSession) -> Result<ValidationMetrics, IamError> {
        let total_duration = session.started_at.elapsed();
        let memory_peak = session.checkpoints.iter()
            .map(|cp| cp.memory_usage)
            .max()
            .unwrap_or(session.memory_baseline);

        // Record metrics
        let mut metrics = self.metrics.write().await;
        metrics.validation_times.push(total_duration);
        metrics.memory_usage_samples.push(memory_peak);

        // Check for performance alerts
        let alerts = self.check_performance_alerts(&total_duration, memory_peak).await;
        if !alerts.is_empty() {
            self.handle_performance_alerts(alerts).await;
        }

        // Update cache statistics
        self.update_cache_statistics().await;

        Ok(ValidationMetrics {
            validation_time_ms: total_duration.as_millis() as u64,
            memory_usage_bytes: memory_peak,
            validation_steps: session.checkpoints.len() as u32,
            schema_load_time_ms: self.calculate_schema_load_time(&session).as_millis() as u64,
        })
    }

    /// Check if policy result is cached
    pub async fn get_cached_result(&self, policy_content: &str) -> Option<PolicyValidationResult> {
        let cache = self.cache.read().await;
        let policy_hash = self.hash_policy_content(policy_content);
        
        if let Some(cached) = cache.policy_results.get(&policy_hash) {
            if !self.is_cache_entry_expired(&cached.cached_at) {
                // Update access count (would need write lock in real implementation)
                return Some(cached.result.clone());
            }
        }
        None
    }

    /// Cache policy validation result
    pub async fn cache_result(&self, policy_content: &str, result: PolicyValidationResult) {
        let mut cache = self.cache.write().await;
        let policy_hash = self.hash_policy_content(policy_content);
        
        // Implement LRU eviction if cache is full
        if cache.policy_results.len() >= cache.max_entries {
            self.evict_lru_entries(&mut cache);
        }

        cache.policy_results.insert(policy_hash, CachedValidationResult {
            result,
            cached_at: Instant::now(),
            access_count: 1,
        });
    }

    /// Get cached schema
    pub async fn get_cached_schema(&self, schema_version: &str) -> Option<(SchemaValidationInfo, String)> {
        let cache = self.cache.read().await;
        
        if let Some(cached) = cache.schema_cache.get(schema_version) {
            if !self.is_cache_entry_expired(&cached.cached_at) {
                return Some((cached.schema_info.clone(), cached.schema_content.clone()));
            }
        }
        None
    }

    /// Cache schema information
    pub async fn cache_schema(&self, schema_version: String, schema_info: SchemaValidationInfo, schema_content: String) {
        let mut cache = self.cache.write().await;
        
        cache.schema_cache.insert(schema_version, CachedSchema {
            schema_info,
            schema_content,
            cached_at: Instant::now(),
            access_count: 1,
        });
    }

    /// Record batch processing metrics
    pub async fn record_batch_metrics(&self, batch_size: usize, processing_time: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.batch_processing_times.push(processing_time);
        
        let throughput = batch_size as f64 / processing_time.as_secs_f64();
        metrics.throughput_samples.push(throughput);
    }

    /// Record validation error
    pub async fn record_validation_error(&self, error_type: ValidationErrorType) {
        let mut metrics = self.metrics.write().await;
        *metrics.error_rates.entry(error_type).or_insert(0) += 1;
    }

    /// Get performance statistics
    pub async fn get_performance_statistics(&self) -> PerformanceStatistics {
        let metrics = self.metrics.read().await;
        let cache = self.cache.read().await;

        PerformanceStatistics {
            avg_validation_time_ms: self.calculate_average_duration(&metrics.validation_times),
            p95_validation_time_ms: self.calculate_percentile_duration(&metrics.validation_times, 0.95),
            avg_memory_usage_mb: self.calculate_average_memory(&metrics.memory_usage_samples),
            cache_hit_rate: metrics.cache_hit_rate,
            total_validations: metrics.validation_times.len(),
            total_cache_entries: cache.policy_results.len() + cache.schema_cache.len(),
            error_distribution: metrics.error_rates.clone(),
            avg_throughput_pps: self.calculate_average_throughput(&metrics.throughput_samples),
        }
    }

    /// Optimize cache based on usage patterns
    pub async fn optimize_cache(&self) {
        let mut cache = self.cache.write().await;
        
        // Remove expired entries
        let now = Instant::now();
        cache.policy_results.retain(|_, cached| {
            now.duration_since(cached.cached_at).as_secs() < cache.ttl_seconds
        });
        
        cache.schema_cache.retain(|_, cached| {
            now.duration_since(cached.cached_at).as_secs() < cache.ttl_seconds
        });

        // Adjust cache size based on hit rate
        if cache.policy_results.len() > cache.max_entries / 2 {
            // If cache is more than half full, consider increasing size or being more aggressive with eviction
            self.evict_lru_entries(&mut cache);
        }
    }

    /// Clear performance metrics (for testing or reset)
    pub async fn clear_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = PerformanceMetrics::new();
    }

    // Private helper methods

    fn hash_policy_content(&self, content: &str) -> String {
        // Simple hash implementation - in production, use a proper hash function
        format!("{:x}", content.len() + content.chars().map(|c| c as usize).sum::<usize>())
    }

    fn is_cache_entry_expired(&self, cached_at: &Instant) -> bool {
        cached_at.elapsed().as_secs() > self.cache.blocking_read().ttl_seconds
    }

    fn evict_lru_entries(&self, cache: &mut ValidationCache) {
        // Simple LRU implementation - remove entries with lowest access count
        if cache.policy_results.len() >= cache.max_entries {
            let mut entries: Vec<_> = cache.policy_results.iter().collect();
            entries.sort_by_key(|(_, cached)| cached.access_count);
            
            // Remove 10% of entries
            let to_remove = cache.max_entries / 10;
            for (key, _) in entries.iter().take(to_remove) {
                cache.policy_results.remove(*key);
            }
        }
    }

    async fn check_performance_alerts(&self, duration: &Duration, memory_usage: usize) -> Vec<PerformanceAlert> {
        let mut alerts = Vec::new();

        // Check validation time
        let duration_ms = duration.as_millis() as u64;
        if duration_ms > self.thresholds.max_validation_time_ms {
            alerts.push(PerformanceAlert::ValidationTimeExceeded {
                actual_ms: duration_ms,
                threshold_ms: self.thresholds.max_validation_time_ms,
            });
        }

        // Check memory usage
        let memory_mb = memory_usage / (1024 * 1024);
        if memory_mb > self.thresholds.max_memory_usage_mb {
            alerts.push(PerformanceAlert::MemoryUsageExceeded {
                actual_mb: memory_mb,
                threshold_mb: self.thresholds.max_memory_usage_mb,
            });
        }

        // Check cache hit rate
        let metrics = self.metrics.read().await;
        if metrics.cache_hit_rate < self.thresholds.min_cache_hit_rate {
            alerts.push(PerformanceAlert::CacheHitRateLow {
                actual_rate: metrics.cache_hit_rate,
                threshold_rate: self.thresholds.min_cache_hit_rate,
            });
        }

        alerts
    }

    async fn handle_performance_alerts(&self, alerts: Vec<PerformanceAlert>) {
        for alert in alerts {
            match alert {
                PerformanceAlert::ValidationTimeExceeded { actual_ms, threshold_ms } => {
                    tracing::warn!(
                        "Validation time exceeded threshold: {}ms > {}ms",
                        actual_ms, threshold_ms
                    );
                }
                PerformanceAlert::MemoryUsageExceeded { actual_mb, threshold_mb } => {
                    tracing::warn!(
                        "Memory usage exceeded threshold: {}MB > {}MB",
                        actual_mb, threshold_mb
                    );
                }
                PerformanceAlert::CacheHitRateLow { actual_rate, threshold_rate } => {
                    tracing::warn!(
                        "Cache hit rate below threshold: {:.2}% < {:.2}%",
                        actual_rate * 100.0, threshold_rate * 100.0
                    );
                }
                _ => {}
            }
        }
    }

    async fn update_cache_statistics(&self) {
        // Update cache hit rate and other statistics
        let cache = self.cache.read().await;
        let total_accesses: usize = cache.policy_results.values().map(|c| c.access_count).sum();
        let cache_hits = cache.policy_results.len();
        
        if total_accesses > 0 {
            let hit_rate = cache_hits as f64 / total_accesses as f64;
            let mut metrics = self.metrics.write().await;
            metrics.cache_hit_rate = hit_rate;
        }
    }

    fn calculate_schema_load_time(&self, session: &PerformanceSession) -> Duration {
        // Find schema loading checkpoint
        session.checkpoints.iter()
            .find(|cp| cp.name.contains("schema"))
            .map(|cp| cp.duration_from_start)
            .unwrap_or_default()
    }

    fn get_current_memory_usage(&self) -> usize {
        // Placeholder implementation - in production, use proper memory tracking
        1024 * 1024 // 1MB placeholder
    }

    fn calculate_average_duration(&self, durations: &[Duration]) -> u64 {
        if durations.is_empty() {
            return 0;
        }
        let total_ms: u64 = durations.iter().map(|d| d.as_millis() as u64).sum();
        total_ms / durations.len() as u64
    }

    fn calculate_percentile_duration(&self, durations: &[Duration], percentile: f64) -> u64 {
        if durations.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = durations.iter().map(|d| d.as_millis() as u64).collect();
        sorted.sort();
        let index = ((durations.len() as f64 * percentile) as usize).min(durations.len() - 1);
        sorted[index]
    }

    fn calculate_average_memory(&self, samples: &[usize]) -> usize {
        if samples.is_empty() {
            return 0;
        }
        samples.iter().sum::<usize>() / samples.len()
    }

    fn calculate_average_throughput(&self, samples: &[f64]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        samples.iter().sum::<f64>() / samples.len() as f64
    }
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            validation_times: Vec::new(),
            memory_usage_samples: Vec::new(),
            cache_hit_rate: 0.0,
            schema_load_times: Vec::new(),
            batch_processing_times: Vec::new(),
            error_rates: HashMap::new(),
            throughput_samples: Vec::new(),
        }
    }
}

impl ValidationCache {
    pub fn new(max_entries: usize, ttl_seconds: u64) -> Self {
        Self {
            policy_results: HashMap::new(),
            schema_cache: HashMap::new(),
            max_entries,
            ttl_seconds,
        }
    }
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            max_validation_time_ms: 1000,      // 1 second
            max_memory_usage_mb: 100,          // 100 MB
            min_cache_hit_rate: 0.8,           // 80%
            max_schema_load_time_ms: 500,      // 500ms
            max_batch_processing_time_ms: 5000, // 5 seconds
            min_throughput_policies_per_sec: 10.0, // 10 policies/sec
        }
    }
}

/// Performance statistics for reporting
#[derive(Debug, Clone)]
pub struct PerformanceStatistics {
    pub avg_validation_time_ms: u64,
    pub p95_validation_time_ms: u64,
    pub avg_memory_usage_mb: usize,
    pub cache_hit_rate: f64,
    pub total_validations: usize,
    pub total_cache_entries: usize,
    pub error_distribution: HashMap<ValidationErrorType, usize>,
    pub avg_throughput_pps: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_performance_monitor_creation() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        let stats = monitor.get_performance_statistics().await;
        assert_eq!(stats.total_validations, 0);
        assert_eq!(stats.cache_hit_rate, 0.0);
    }

    #[tokio::test]
    async fn test_performance_session() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        let mut session = monitor.start_session("test-session".to_string()).await;
        assert_eq!(session.session_id, "test-session");
        assert!(session.checkpoints.is_empty());
        
        monitor.add_checkpoint(&mut session, "checkpoint1".to_string()).await;
        assert_eq!(session.checkpoints.len(), 1);
        assert_eq!(session.checkpoints[0].name, "checkpoint1");
        
        let metrics = monitor.finish_session(session).await.unwrap();
        assert!(metrics.validation_time_ms > 0);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        let policy_content = "permit(principal, action, resource);";
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
        
        // Initially no cached result
        assert!(monitor.get_cached_result(policy_content).await.is_none());
        
        // Cache the result
        monitor.cache_result(policy_content, result.clone()).await;
        
        // Should now find cached result
        let cached = monitor.get_cached_result(policy_content).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().schema_info.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_batch_metrics() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        let batch_size = 10;
        let processing_time = Duration::from_millis(500);
        
        monitor.record_batch_metrics(batch_size, processing_time).await;
        
        let stats = monitor.get_performance_statistics().await;
        assert_eq!(stats.avg_throughput_pps, 20.0); // 10 policies / 0.5 seconds
    }

    #[tokio::test]
    async fn test_error_recording() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        monitor.record_validation_error(ValidationErrorType::SyntaxError).await;
        monitor.record_validation_error(ValidationErrorType::SyntaxError).await;
        monitor.record_validation_error(ValidationErrorType::SemanticError).await;
        
        let stats = monitor.get_performance_statistics().await;
        assert_eq!(stats.error_distribution.get(&ValidationErrorType::SyntaxError), Some(&2));
        assert_eq!(stats.error_distribution.get(&ValidationErrorType::SemanticError), Some(&1));
    }

    #[tokio::test]
    async fn test_cache_optimization() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PolicyValidationPerformanceMonitor::new(thresholds);
        
        // Add some cache entries
        for i in 0..5 {
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
}