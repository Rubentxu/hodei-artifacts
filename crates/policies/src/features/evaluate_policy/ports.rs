//! Segregated interfaces for policy evaluation feature
//! 
//! This module defines the specific interfaces (ports) required by the policy evaluation feature.
//! Following SOLID principles, particularly Interface Segregation Principle.

use crate::domain::context::PolicyEvaluationContext;
use crate::domain::decision::PolicyDecision;
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::PolicyId;
use std::sync::Arc;

/// Port for policy storage operations
#[async_trait]
pub trait PolicyStoragePort: Send + Sync {
    /// Retrieve a policy by ID
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, EvaluatePolicyError>;

    /// Store a policy
    async fn store_policy(&self, policy: &Policy) -> Result<(), EvaluatePolicyError>;

    /// Find policies applicable to a context
    async fn find_applicable_policies(
        &self,
        context: &PolicyEvaluationContext,
    ) -> Result<Vec<Policy>, EvaluatePolicyError>;

    /// Check if a policy exists
    async fn policy_exists(&self, policy_id: &PolicyId) -> Result<bool, EvaluatePolicyError>;

    /// Get policy version for caching
    async fn get_policy_version(&self, policy_id: &PolicyId) -> Result<Option<String>, EvaluatePolicyError>;

    /// Convenience: create a policy (alias of store_policy). Default implementation calls store_policy.
    async fn create_policy(&self, policy: &Policy) -> Result<(), EvaluatePolicyError> {
        self.store_policy(policy).await
    }

    /// Convenience: list all policies. Default returns empty; adapters can override.
    async fn list_policies(&self) -> Result<Vec<Policy>, EvaluatePolicyError> {
        Ok(Vec::new())
    }
}

/// Port for Cedar policy engine operations
#[async_trait]
pub trait CedarEnginePort: Send + Sync {
    /// Compile a Cedar policy
    async fn compile_policy(&self, policy_content: &str) -> Result<Arc<dyn CompiledPolicy>, EvaluatePolicyError>;

    /// Evaluate a compiled policy against context
    async fn evaluate_policy(
        &self,
        compiled_policy: &dyn CompiledPolicy,
        context: &PolicyEvaluationContext,
    ) -> Result<PolicyDecision, EvaluatePolicyError>;

    /// Validate policy syntax
    async fn validate_policy_syntax(&self, policy_content: &str) -> Result<(), EvaluatePolicyError>;

    /// Get engine capabilities and metrics
    async fn get_engine_metrics(&self) -> Result<EngineMetrics, EvaluatePolicyError>;
}

/// Port for compiled policy representation
pub trait CompiledPolicy: Send + Sync {
    /// Get the policy ID
    fn policy_id(&self) -> &str;

    /// Get the policy content
    fn content(&self) -> &str;

    /// Get compilation timestamp
    fn compiled_at(&self) -> chrono::DateTime<chrono::Utc>;

    /// Get policy hash for caching
    fn hash(&self) -> &str;
}

/// Port for caching policy evaluation results
#[async_trait]
pub trait PolicyCachePort: Send + Sync {
    /// Get cached evaluation result
    async fn get_cached_result(
        &self,
        cache_key: &str,
    ) -> Result<Option<CachedEvaluationResult>, EvaluatePolicyError>;

    /// Cache evaluation result
    async fn cache_result(
        &self,
        cache_key: &str,
        result: &CachedEvaluationResult,
        ttl_seconds: u64,
    ) -> Result<(), EvaluatePolicyError>;

    /// Invalidate cache for a policy
    async fn invalidate_policy_cache(&self, policy_id: &PolicyId) -> Result<(), EvaluatePolicyError>;

    /// Get cache statistics
    async fn get_cache_stats(&self) -> Result<CacheStats, EvaluatePolicyError>;
}

/// Port for performance monitoring
#[async_trait]
pub trait PerformanceMonitorPort: Send + Sync {
    /// Record evaluation metrics
    async fn record_evaluation(&self, metrics: EvaluationMetrics) -> Result<(), EvaluatePolicyError>;

    /// Record error metrics
    async fn record_error(&self, error: &EvaluatePolicyError, context: &str) -> Result<(), EvaluatePolicyError>;

    /// Get performance summary
    async fn get_performance_summary(&self) -> Result<PerformanceSummary, EvaluatePolicyError>;
}

/// Port for event publishing
#[async_trait]
pub trait EventPublisherPort: Send + Sync {
    /// Publish policy evaluation event
    async fn publish_evaluation_event(&self, event: PolicyEvaluationEvent) -> Result<(), EvaluatePolicyError>;

    /// Publish policy decision event
    async fn publish_decision_event(&self, event: PolicyDecisionEvent) -> Result<(), EvaluatePolicyError>;

    /// Publish performance metrics event
    async fn publish_metrics_event(&self, event: MetricsEvent) -> Result<(), EvaluatePolicyError>;
}

/// Import the required types
use super::{dto::*, error::EvaluatePolicyError};


/// Engine capabilities and metrics
#[derive(Debug, Clone)]
pub struct EngineMetrics {
    pub policies_loaded: u32,
    pub compilation_cache_size: u32,
    pub evaluation_cache_size: u32,
    pub memory_usage_bytes: u64,
    pub uptime_seconds: u64,
}

/// Cached evaluation result
#[derive(Debug, Clone)]
pub struct CachedEvaluationResult {
    pub decision: PolicyDecision,
    pub cached_at: chrono::DateTime<chrono::Utc>,
    pub ttl_seconds: u64,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: u32,
    pub hit_count: u32,
    pub miss_count: u32,
    pub hit_rate: f64,
    pub memory_usage_bytes: u64,
}

/// Performance summary
#[derive(Debug, Clone)]
pub struct PerformanceSummary {
    pub total_evaluations: u64,
    pub average_time_ms: f64,
    pub success_rate: f64,
    pub cache_hit_rate: f64,
    pub error_rate: f64,
}

/// Policy evaluation event
#[derive(Debug, Clone)]
pub struct PolicyEvaluationEvent {
    pub policy_id: PolicyId,
    pub context: PolicyEvaluationContext,
    pub decision: PolicyDecision,
    pub evaluation_time_ms: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Policy decision event
#[derive(Debug, Clone)]
pub struct PolicyDecisionEvent {
    pub policy_id: PolicyId,
    pub decision: PolicyDecision,
    pub context_hash: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Performance metrics event
#[derive(Debug, Clone)]
pub struct MetricsEvent {
    pub metrics: EvaluationMetrics,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}