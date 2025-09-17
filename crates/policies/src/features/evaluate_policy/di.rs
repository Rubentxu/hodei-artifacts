//! Dependency injection configuration for policy evaluation feature
//! 
//! This module provides dependency injection setup for all ports and adapters.

use crate::features::evaluate_policy::ports::*;
use crate::features::evaluate_policy::{EvaluatePolicyError, EvaluationMetrics};
use async_trait::async_trait;
use std::sync::Arc;

/// Container for all policy evaluation dependencies
pub struct PolicyEvaluationContainer {
    pub policy_storage: Arc<dyn PolicyStoragePort>,
    pub cedar_engine: Arc<dyn CedarEnginePort>,
    pub policy_cache: Arc<dyn PolicyCachePort>,
    pub performance_monitor: Arc<dyn PerformanceMonitorPort>,
    pub event_publisher: Arc<dyn EventPublisherPort>,
}

impl PolicyEvaluationContainer {
    /// Create a new container with production implementations
    pub fn new_production(
        db_connection: Arc<surrealdb::Surreal<surrealdb::engine::any::Any>>,
    policy_table_name: String,
    cache_ttl_seconds: u64,
    max_cache_entries: u32,
    enable_metrics: bool,
        enable_events: bool,
    ) -> Self {
        let policy_storage = Arc::new(
            super::adapter::PolicyStorageAdapter::new(db_connection, policy_table_name)
        );
        
        let cedar_engine = Arc::new(super::adapter::CedarEngineAdapter::new());
        
        let policy_cache = Arc::new(super::adapter::InMemoryCacheAdapter::new(1000, std::time::Duration::from_secs(cache_ttl_seconds)));
        
        let performance_monitor = if enable_metrics {
            Arc::new(super::adapter::LoggingPerformanceMonitor) as Arc<dyn PerformanceMonitorPort>
        } else {
            Arc::new(NoOpPerformanceMonitor) as Arc<dyn PerformanceMonitorPort>
        };
        
        let event_publisher = if enable_events {
            Arc::new(super::adapter::LoggingEventPublisher) as Arc<dyn EventPublisherPort>
        } else {
            Arc::new(NoOpEventPublisher) as Arc<dyn EventPublisherPort>
        };
        
        Self {
            policy_storage,
            cedar_engine,
            policy_cache,
            performance_monitor,
            event_publisher,
        }
    }

    /// Create a new container with testing implementations
    pub fn new_testing() -> Self {
        Self {
            policy_storage: Arc::new(super::mocks::MockPolicyStoragePort::new()),
            cedar_engine: Arc::new(super::mocks::MockCedarEnginePort::new()),
            policy_cache: Arc::new(super::mocks::MockPolicyCachePort::new()),
            performance_monitor: Arc::new(super::mocks::MockPerformanceMonitorPort::new()),
            event_publisher: Arc::new(super::mocks::MockEventPublisherPort::new()),
        }
    }

    /// Create a new container with custom implementations
    pub fn new_custom(
        policy_storage: Arc<dyn PolicyStoragePort>,
        cedar_engine: Arc<dyn CedarEnginePort>,
        policy_cache: Arc<dyn PolicyCachePort>,
        performance_monitor: Arc<dyn PerformanceMonitorPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        Self {
            policy_storage,
            cedar_engine,
            policy_cache,
            performance_monitor,
            event_publisher,
        }
    }
}

/// No-op performance monitor for testing or when metrics are disabled
pub struct NoOpPerformanceMonitor;

#[async_trait]
impl PerformanceMonitorPort for NoOpPerformanceMonitor {
    async fn record_evaluation(&self, _metrics: EvaluationMetrics) -> Result<(), EvaluatePolicyError> {
        Ok(())
    }

    async fn record_error(&self, _error: &EvaluatePolicyError, _context: &str) -> Result<(), EvaluatePolicyError> {
        Ok(())
    }

    async fn get_performance_summary(&self) -> Result<PerformanceSummary, EvaluatePolicyError> {
        Ok(PerformanceSummary {
            total_evaluations: 0,
            average_time_ms: 0.0,
            success_rate: 0.0,
            cache_hit_rate: 0.0,
            error_rate: 0.0,
        })
    }
}

/// No-op event publisher for testing or when events are disabled
pub struct NoOpEventPublisher;

#[async_trait]
impl EventPublisherPort for NoOpEventPublisher {
    async fn publish_evaluation_event(&self, _event: PolicyEvaluationEvent) -> Result<(), EvaluatePolicyError> {
        Ok(())
    }

    async fn publish_decision_event(&self, _event: PolicyDecisionEvent) -> Result<(), EvaluatePolicyError> {
        Ok(())
    }

    async fn publish_metrics_event(&self, _event: MetricsEvent) -> Result<(), EvaluatePolicyError> {
        Ok(())
    }
}

/// Builder pattern for creating dependency containers
pub struct PolicyEvaluationContainerBuilder {
    db_connection: Option<Arc<surrealdb::Surreal<surrealdb::engine::any::Any>>>,
    policy_table_name: String,
    cache_ttl_seconds: u64,
    max_cache_entries: u32,
    enable_metrics: bool,
    enable_events: bool,
}

impl PolicyEvaluationContainerBuilder {
    /// Create a new builder with default values
    pub fn new() -> Self {
        Self {
            db_connection: None,
            policy_table_name: "policies".to_string(),
            cache_ttl_seconds: 300, // 5 minutes
            max_cache_entries: 1000,
            enable_metrics: true,
            enable_events: true,
        }
    }

    /// Set the database connection
    pub fn with_db_connection(mut self, db_connection: Arc<surrealdb::Surreal<surrealdb::engine::any::Any>>) -> Self {
        self.db_connection = Some(db_connection);
        self
    }

    /// Set the policy table name
    pub fn with_policy_table_name(mut self, table_name: impl Into<String>) -> Self {
        self.policy_table_name = table_name.into();
        self
    }

    /// Set the cache TTL in seconds
    pub fn with_cache_ttl(mut self, ttl_seconds: u64) -> Self {
        self.cache_ttl_seconds = ttl_seconds;
        self
    }

    /// Set the maximum cache entries
    pub fn with_max_cache_entries(mut self, max_entries: u32) -> Self {
        self.max_cache_entries = max_entries;
        self
    }

    /// Enable or disable metrics collection
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Enable or disable event publishing
    pub fn with_events(mut self, enable: bool) -> Self {
        self.enable_events = enable;
        self
    }

    /// Build the dependency container
    pub fn build(self) -> Result<PolicyEvaluationContainer, String> {
        let db_connection = self.db_connection.ok_or_else(|| {
            "Database connection is required for production container".to_string()
        })?;

        Ok(PolicyEvaluationContainer::new_production(
            db_connection,
            self.policy_table_name,
            self.cache_ttl_seconds,
            self.max_cache_entries,
            self.enable_metrics,
            self.enable_events,
        ))
    }
}

impl Default for PolicyEvaluationContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}