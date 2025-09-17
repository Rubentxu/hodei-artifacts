//! Mock implementations for testing policy evaluation feature
//! 
//! This module provides mock implementations of all ports for unit testing.

use super::dto::*;
use super::error::EvaluatePolicyError;
use crate::domain::context::PolicyEvaluationContext;
use crate::domain::decision::PolicyDecision;
use crate::domain::policy::Policy;
use crate::features::evaluate_policy::ports::*;
use async_trait::async_trait;
use mockall::mock;
use shared::hrn::PolicyId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::domain::action::Action;
use shared::hrn::{Hrn, UserId};
use std::str::FromStr;

// Mock implementations using mockall for each port

mock! {
    pub PolicyStoragePort {}
    
    #[async_trait]
    impl PolicyStoragePort for PolicyStoragePort {
        async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, EvaluatePolicyError>;
        async fn store_policy(&self, policy: &Policy) -> Result<(), EvaluatePolicyError>;
        async fn find_applicable_policies(&self, context: &PolicyEvaluationContext) -> Result<Vec<Policy>, EvaluatePolicyError>;
        async fn policy_exists(&self, policy_id: &PolicyId) -> Result<bool, EvaluatePolicyError>;
        async fn get_policy_version(&self, policy_id: &PolicyId) -> Result<Option<String>, EvaluatePolicyError>;
    }
}

/// Test data builder utilities for evaluation tests
pub struct TestDataBuilder;

impl TestDataBuilder {
    /// Build a sample evaluation context with sane defaults
    pub fn sample_evaluation_context() -> PolicyEvaluationContext {
        let principal = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let action = Action::ReadArtifact;
        let resource = Hrn::new("hrn:hodei:artifact::physical-artifact/sha256-abcd1234").unwrap();
        let time = chrono::Utc::now();

        let mut ctx = PolicyEvaluationContext::new(principal, action, resource, time);
        // add common attributes
        ctx.additional_attributes.insert(
            "principal_department".to_string(),
            shared::attributes::AttributeValue::String("engineering".to_string()),
        );
        ctx.additional_attributes.insert(
            "resource_department".to_string(),
            shared::attributes::AttributeValue::String("engineering".to_string()),
        );
        ctx
    }
}

// Manual mock implementation
#[derive(Default)]
pub struct MockCedarEnginePort {
    should_succeed: Arc<RwLock<bool>>,
    evaluation_delay_ms: Arc<RwLock<u64>>,
}

#[async_trait]
impl CedarEnginePort for MockCedarEnginePort {
    async fn compile_policy(&self, policy_content: &str) -> Result<Arc<dyn CompiledPolicy>, EvaluatePolicyError> {
        if policy_content.contains("invalid") {
            return Err(EvaluatePolicyError::PolicyCompilationError(
                "Invalid policy syntax".to_string()
            ));
        }

        let compiled = Arc::new(MockCompiledPolicy {
            content: policy_content.to_string(),
            compiled_at: chrono::Utc::now(),
            hash: format!("hash:{}", policy_content.len()),
        });

        Ok(compiled)
    }

    async fn evaluate_policy(
        &self,
        _compiled_policy: &dyn CompiledPolicy,
        _context: &PolicyEvaluationContext,
    ) -> Result<PolicyDecision, EvaluatePolicyError> {
        // Simulate evaluation delay
        let delay = *self.evaluation_delay_ms.read().await;
        if delay > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }

        let should_succeed = *self.should_succeed.read().await;
        if should_succeed {
            Ok(PolicyDecision {
                allowed: true,
                reasons: vec!["Mock policy allows access".to_string()],
                obligations: vec![],
                advice: vec![],
            })
        } else {
            Ok(PolicyDecision {
                allowed: false,
                reasons: vec!["Mock policy denies access".to_string()],
                obligations: vec![],
                advice: vec![],
            })
        }
    }

    async fn validate_policy_syntax(&self, policy_content: &str) -> Result<(), EvaluatePolicyError> {
        if policy_content.contains("syntax_error") {
            return Err(EvaluatePolicyError::InvalidPolicySyntax(
                "Syntax error detected".to_string()
            ));
        }
        Ok(())
    }

    async fn get_engine_metrics(&self) -> Result<EngineMetrics, EvaluatePolicyError> {
        Ok(EngineMetrics {
            policies_loaded: 10,
            compilation_cache_size: 5,
            evaluation_cache_size: 20,
            memory_usage_bytes: 1024,
            uptime_seconds: 3600,
        })
    }
}

/// Mock compiled policy
pub struct MockCompiledPolicy {
    content: String,
    compiled_at: chrono::DateTime<chrono::Utc>,
    hash: String,
}

impl CompiledPolicy for MockCompiledPolicy {
    fn policy_id(&self) -> &str {
        "mock-policy"
    }

    fn content(&self) -> &str {
        &self.content
    }

    fn compiled_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.compiled_at
    }

    fn hash(&self) -> &str {
        &self.hash
    }


}

mock! {
    pub PolicyCachePort {}
    
    #[async_trait]
    impl PolicyCachePort for PolicyCachePort {
        async fn get_cached_result(&self, cache_key: &str) -> Result<Option<CachedEvaluationResult>, EvaluatePolicyError>;
        async fn cache_result(&self, cache_key: &str, result: &CachedEvaluationResult, ttl_seconds: u64) -> Result<(), EvaluatePolicyError>;
        async fn invalidate_policy_cache(&self, policy_id: &PolicyId) -> Result<(), EvaluatePolicyError>;
        async fn get_cache_stats(&self) -> Result<CacheStats, EvaluatePolicyError>;
    }
}

mock! {
    pub PerformanceMonitorPort {}
    
    #[async_trait]
    impl PerformanceMonitorPort for PerformanceMonitorPort {
        async fn record_evaluation(&self, metrics: EvaluationMetrics) -> Result<(), EvaluatePolicyError>;
        async fn record_error(&self, error: &EvaluatePolicyError, context: &str) -> Result<(), EvaluatePolicyError>;
        async fn get_performance_summary(&self) -> Result<PerformanceSummary, EvaluatePolicyError>;
    }
}

mock! {
    pub EventPublisherPort {}
    
    #[async_trait]
    impl EventPublisherPort for EventPublisherPort {
        async fn publish_evaluation_event(&self, event: PolicyEvaluationEvent) -> Result<(), EvaluatePolicyError>;
        async fn publish_decision_event(&self, event: PolicyDecisionEvent) -> Result<(), EvaluatePolicyError>;
        async fn publish_metrics_event(&self, event: MetricsEvent) -> Result<(), EvaluatePolicyError>;
    }
}

// In-memory mock implementations for testing

/// In-memory mock policy storage
pub struct InMemoryPolicyStorage {
    policies: Arc<RwLock<HashMap<String, Policy>>>,
}

impl InMemoryPolicyStorage {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_policy(&self, policy: Policy) {
        let mut policies = self.policies.write().await;
        policies.insert(policy.id.to_string(), policy);
    }

    pub async fn clear(&self) {
        let mut policies = self.policies.write().await;
        policies.clear();
    }
}

#[async_trait]
impl PolicyStoragePort for InMemoryPolicyStorage {
    async fn get_policy(&self, policy_id: &PolicyId) -> Result<Option<Policy>, EvaluatePolicyError> {
        let policies = self.policies.read().await;
        Ok(policies.get(&policy_id.to_string()).cloned())
    }

    async fn store_policy(&self, policy: &Policy) -> Result<(), EvaluatePolicyError> {
        let mut policies = self.policies.write().await;
        policies.insert(policy.id.to_string(), policy.clone());
        Ok(())
    }

    async fn find_applicable_policies(&self, _context: &PolicyEvaluationContext) -> Result<Vec<Policy>, EvaluatePolicyError> {
        // Return all policies for simplicity in testing
        let policies = self.policies.read().await;
        Ok(policies.values().cloned().collect())
    }

    async fn policy_exists(&self, policy_id: &PolicyId) -> Result<bool, EvaluatePolicyError> {
        let policies = self.policies.read().await;
        Ok(policies.contains_key(&policy_id.to_string()))
    }

    async fn get_policy_version(&self, policy_id: &PolicyId) -> Result<Option<String>, EvaluatePolicyError> {
        let policies = self.policies.read().await;
        Ok(policies.get(&policy_id.to_string()).map(|p| p.version.to_string()))
    }
}

/// Mock cache implementation
pub struct MockPolicyCache {
    cache: Arc<RwLock<HashMap<String, CachedEvaluationResult>>>,
    should_hit: Arc<RwLock<bool>>,
}

impl MockPolicyCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            should_hit: Arc::new(RwLock::new(true)),
        }
    }

    pub async fn set_should_hit(&self, hit: bool) {
        let mut should_hit = self.should_hit.write().await;
        *should_hit = hit;
    }

    pub async fn store(&self, key: String, result: CachedEvaluationResult) {
        let mut cache = self.cache.write().await;
        cache.insert(key, result);
    }
}

#[async_trait]
impl PolicyCachePort for MockPolicyCache {
    async fn get_cached_result(&self, cache_key: &str) -> Result<Option<CachedEvaluationResult>, EvaluatePolicyError> {
        let should_hit = *self.should_hit.read().await;
        if should_hit {
            let cache = self.cache.read().await;
            Ok(cache.get(cache_key).cloned())
        } else {
            Ok(None)
        }
    }

    async fn cache_result(&self, cache_key: &str, result: &CachedEvaluationResult, _ttl_seconds: u64) -> Result<(), EvaluatePolicyError> {
        let mut cache = self.cache.write().await;
        cache.insert(cache_key.to_string(), result.clone());
        Ok(())
    }

    async fn invalidate_policy_cache(&self, _policy_id: &PolicyId) -> Result<(), EvaluatePolicyError> {
        let mut cache = self.cache.write().await;
        cache.clear();
        Ok(())
    }

    async fn get_cache_stats(&self) -> Result<CacheStats, EvaluatePolicyError> {
        let cache = self.cache.read().await;
        Ok(CacheStats {
            total_entries: cache.len() as u32,
            hit_count: 10,
            miss_count: 5,
            hit_rate: 0.666,
            memory_usage_bytes: 512,
        })
    }
}

/// Mock performance monitor
pub struct MockPerformanceMonitor {
    evaluations: Arc<RwLock<Vec<EvaluationMetrics>>>,
    errors: Arc<RwLock<Vec<(String, String)>>>,
}

impl MockPerformanceMonitor {
    pub fn new() -> Self {
        Self {
            evaluations: Arc::new(RwLock::new(Vec::new())),
            errors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn get_evaluations(&self) -> Vec<EvaluationMetrics> {
        let evaluations = self.evaluations.read().await;
        evaluations.clone()
    }

    pub async fn get_errors(&self) -> Vec<(String, String)> {
        let errors = self.errors.read().await;
        errors.clone()
    }

    pub async fn clear(&self) {
        let mut evaluations = self.evaluations.write().await;
        let mut errors = self.errors.write().await;
        evaluations.clear();
        errors.clear();
    }
}

#[async_trait]
impl PerformanceMonitorPort for MockPerformanceMonitor {
    async fn record_evaluation(&self, metrics: EvaluationMetrics) -> Result<(), EvaluatePolicyError> {
        let mut evaluations = self.evaluations.write().await;
        evaluations.push(metrics);
        Ok(())
    }

    async fn record_error(&self, error: &EvaluatePolicyError, context: &str) -> Result<(), EvaluatePolicyError> {
        let mut errors = self.errors.write().await;
        errors.push((error.error_category().to_string(), context.to_string()));
        Ok(())
    }

    async fn get_performance_summary(&self) -> Result<PerformanceSummary, EvaluatePolicyError> {
        Ok(PerformanceSummary {
            total_evaluations: 100,
            average_time_ms: 25.5,
            success_rate: 0.98,
            cache_hit_rate: 0.75,
            error_rate: 0.02,
        })
    }
}

/// Mock event publisher
pub struct MockEventPublisher {
    events: Arc<RwLock<Vec<String>>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn get_events(&self) -> Vec<String> {
        let events = self.events.read().await;
        events.clone()
    }

    pub async fn clear(&self) {
        let mut events = self.events.write().await;
        events.clear();
    }
}

#[async_trait]
impl EventPublisherPort for MockEventPublisher {
    async fn publish_evaluation_event(&self, event: PolicyEvaluationEvent) -> Result<(), EvaluatePolicyError> {
        let mut events = self.events.write().await;
        events.push(format!("Evaluation: {} -> {}", event.policy_id, event.decision.allowed));
        Ok(())
    }

    async fn publish_decision_event(&self, event: PolicyDecisionEvent) -> Result<(), EvaluatePolicyError> {
        let mut events = self.events.write().await;
        events.push(format!("Decision: {} -> {}", event.policy_id, event.decision.allowed));
        Ok(())
    }

    async fn publish_metrics_event(&self, event: MetricsEvent) -> Result<(), EvaluatePolicyError> {
        let mut events = self.events.write().await;
        events.push(format!("Metrics: {}ms", event.metrics.evaluation_time_ms));
        Ok(())
    }
}
