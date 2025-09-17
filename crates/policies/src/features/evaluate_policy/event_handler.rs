//! Event handlers for policy evaluation feature
//! 
//! This module handles domain events related to policy evaluation.

use super::dto::*;
use super::error::EvaluatePolicyError;
use crate::domain::context::PolicyEvaluationContext;
use crate::domain::decision::PolicyDecision;
use crate::features::evaluate_policy::ports::*;
use async_trait::async_trait;
use shared::hrn::PolicyId;
use shared::policy::events::{DecisionSummary, PolicyEvaluated};
use std::sync::Arc;
use time::OffsetDateTime;
use tracing::log::warn;
use tracing::{debug, error, info, span, Instrument, Level};

/// Event handler for policy evaluation events
pub struct PolicyEvaluationEventHandler {
    event_publisher: Arc<dyn EventPublisherPort>,
    performance_monitor: Arc<dyn PerformanceMonitorPort>,
}

impl PolicyEvaluationEventHandler {
    /// Create a new event handler
    pub fn new(
        event_publisher: Arc<dyn EventPublisherPort>,
        performance_monitor: Arc<dyn PerformanceMonitorPort>,
    ) -> Self {
        Self {
            event_publisher,
            performance_monitor,
        }
    }

    /// Handle policy evaluation completed event
    pub async fn handle_evaluation_completed(
        &self,
        policy_id: PolicyId,
        context: PolicyEvaluationContext,
        decision: PolicyDecision,
        evaluation_time_ms: u64,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_evaluation_completed", policy_id = %policy_id);
        
        async move {
            // Create evaluation event
            let evaluation_event = PolicyEvaluationEvent {
                policy_id: policy_id.clone(),
                context: context.clone(),
                decision: decision.clone(),
                evaluation_time_ms,
                timestamp: chrono::Utc::now(),
            };

            // Create decision event
            let context_hash = self.calculate_context_hash(&context);
            let decision_event = PolicyDecisionEvent {
                policy_id: policy_id.clone(),
                decision: decision.clone(),
                context_hash,
                timestamp: chrono::Utc::now(),
            };

            // Create metrics event
            let metrics = EvaluationMetrics {
                evaluation_time_ms,
                policies_evaluated: 1,
                cache_hit_rate: 0.0, // This would be calculated by the use case
                memory_usage_bytes: 0, // This would be measured
            };
            let metrics_event = MetricsEvent {
                metrics: metrics.clone(),
                timestamp: chrono::Utc::now(),
            };

            // Create shared PolicyEvaluated domain event (shared kernel)
            let policy_evaluated = PolicyEvaluated {
                id: policy_id.clone(),
                context_hash: self.calculate_context_hash(&context),
                decision: DecisionSummary { allowed: decision.allowed },
                evaluation_time_ms,
                occurred_at: OffsetDateTime::now_utc(),
            };

            // Publish events
            self.event_publisher.publish_evaluation_event(evaluation_event).await?;
            self.event_publisher.publish_decision_event(decision_event).await?;
            self.event_publisher.publish_metrics_event(metrics_event).await?;

            // Record metrics
            self.performance_monitor.record_evaluation(metrics).await?;

            // Log shared domain event until bus is wired
            info!(
                policy_id = %policy_evaluated.id,
                decision_allowed = %policy_evaluated.decision.allowed,
                evaluation_time_ms = %policy_evaluated.evaluation_time_ms,
                "PolicyEvaluated domain event created"
            );

            info!(
                policy_id = %policy_id,
                decision_allowed = %decision.allowed,
                evaluation_time_ms = %evaluation_time_ms,
                "Policy evaluation completed and events published"
            );

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle policy evaluation error event
    pub async fn handle_evaluation_error(
        &self,
        error: &EvaluatePolicyError,
        context: &str,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::ERROR, "handle_evaluation_error", error_category = %error.error_category());
        
        async move {
            // Record error metrics
            self.performance_monitor.record_error(error, context).await?;

            error!(
                error = %error,
                error_category = %error.error_category(),
                context = %context,
                "Policy evaluation error handled"
            );

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle policy compilation event
    pub async fn handle_policy_compilation(
        &self,
        policy_id: PolicyId,
        compilation_time_ms: u64,
        success: bool,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_policy_compilation", policy_id = %policy_id);
        
        async move {
            if success {
                info!(
                    policy_id = %policy_id,
                    compilation_time_ms = %compilation_time_ms,
                    "Policy compiled successfully"
                );
            } else {
                error!(
                    policy_id = %policy_id,
                    compilation_time_ms = %compilation_time_ms,
                    "Policy compilation failed"
                );
            }

            // Create and publish compilation metrics event
            let metrics = EvaluationMetrics {
                evaluation_time_ms: compilation_time_ms,
                policies_evaluated: 0,
                cache_hit_rate: 0.0,
                memory_usage_bytes: 0,
            };
            let metrics_event = MetricsEvent {
                metrics,
                timestamp: chrono::Utc::now(),
            };

            self.event_publisher.publish_metrics_event(metrics_event).await?;

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle cache hit event
    pub async fn handle_cache_hit(
        &self,
        policy_id: PolicyId,
        cache_key: &str,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::DEBUG, "handle_cache_hit", policy_id = %policy_id);
        
        async move {
            debug!(
                policy_id = %policy_id,
                cache_key = %cache_key,
                "Policy evaluation cache hit"
            );

            // Record cache hit metrics
            let metrics = EvaluationMetrics {
                evaluation_time_ms: 0, // Cache hits are very fast
                policies_evaluated: 1,
                cache_hit_rate: 1.0,
                memory_usage_bytes: 0,
            };
            self.performance_monitor.record_evaluation(metrics).await?;

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle cache miss event
    pub async fn handle_cache_miss(
        &self,
        policy_id: PolicyId,
        cache_key: &str,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::DEBUG, "handle_cache_miss", policy_id = %policy_id);
        
        async move {
            debug!(
                policy_id = %policy_id,
                cache_key = %cache_key,
                "Policy evaluation cache miss"
            );

            // Record cache miss metrics
            let metrics = EvaluationMetrics {
                evaluation_time_ms: 0, // Will be updated after evaluation
                policies_evaluated: 1,
                cache_hit_rate: 0.0,
                memory_usage_bytes: 0,
            };
            self.performance_monitor.record_evaluation(metrics).await?;

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Calculate hash of evaluation context for event correlation
    fn calculate_context_hash(&self, context: &PolicyEvaluationContext) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        
        hasher.update(context.principal.as_bytes());
        hasher.update(context.action.as_bytes());
        hasher.update(context.resource.resource_id.as_bytes());
        hasher.update(context.organization_id.as_bytes());
        
        // Add environment context to hash
        for (key, value) in &context.environment {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }
        
        format!("{:x}", hasher.finalize())
    }
}

/// Event handler for policy lifecycle events
pub struct PolicyLifecycleEventHandler {
    event_publisher: Arc<dyn EventPublisherPort>,
    performance_monitor: Arc<dyn PerformanceMonitorPort>,
}

impl PolicyLifecycleEventHandler {
    /// Create a new policy lifecycle event handler
    pub fn new(
        event_publisher: Arc<dyn EventPublisherPort>,
        performance_monitor: Arc<dyn PerformanceMonitorPort>,
    ) -> Self {
        Self {
            event_publisher,
            performance_monitor,
        }
    }

    /// Handle policy created event
    pub async fn handle_policy_created(&self, policy_id: PolicyId) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_policy_created", policy_id = %policy_id);
        
        async move {
            info!(policy_id = %policy_id, "Policy created event handled");
            
            // Invalidate cache for the new policy
            // This would typically be done through the cache port
            debug!(policy_id = %policy_id, "Policy cache invalidated for new policy");
            
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle policy updated event
    pub async fn handle_policy_updated(&self, policy_id: PolicyId) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_policy_updated", policy_id = %policy_id);
        
        async move {
            info!(policy_id = %policy_id, "Policy updated event handled");
            
            // Invalidate cache for the updated policy
            debug!(policy_id = %policy_id, "Policy cache invalidated for updated policy");
            
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle policy deleted event
    pub async fn handle_policy_deleted(&self, policy_id: PolicyId) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_policy_deleted", policy_id = %policy_id);
        
        async move {
            info!(policy_id = %policy_id, "Policy deleted event handled");
            
            // Invalidate cache for the deleted policy
            debug!(policy_id = %policy_id, "Policy cache invalidated for deleted policy");
            
            Ok(())
        }
        .instrument(span)
        .await
    }
}

/// Event handler for performance monitoring events
pub struct PerformanceEventHandler {
    performance_monitor: Arc<dyn PerformanceMonitorPort>,
}

impl PerformanceEventHandler {
    /// Create a new performance event handler
    pub fn new(performance_monitor: Arc<dyn PerformanceMonitorPort>) -> Self {
        Self { performance_monitor }
    }

    /// Handle performance threshold exceeded event
    pub async fn handle_performance_threshold_exceeded(
        &self,
        threshold_type: &str,
        actual_value: f64,
        threshold_value: f64,
    ) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::WARN, "handle_performance_threshold_exceeded", threshold_type = %threshold_type);
        
        async move {
            warn!(
                threshold_type = %threshold_type,
                actual_value = %actual_value,
                threshold_value = %threshold_value,
                "Performance threshold exceeded"
            );

            // This could trigger alerts, auto-scaling, or other remediation actions
            // For now, we'll just log the event

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle system health check event
    pub async fn handle_system_health_check(&self) -> Result<(), EvaluatePolicyError> {
        let span = span!(Level::INFO, "handle_system_health_check");
        
        async move {
            // Get performance summary
            let summary = self.performance_monitor.get_performance_summary().await?;
            
            info!(
                total_evaluations = %summary.total_evaluations,
                average_time_ms = %summary.average_time_ms,
                success_rate = %summary.success_rate,
                cache_hit_rate = %summary.cache_hit_rate,
                error_rate = %summary.error_rate,
                "System health check completed"
            );

            // Check for performance issues and trigger alerts if necessary
            if summary.error_rate > 0.05 { // 5% error rate threshold
                self.handle_performance_threshold_exceeded("error_rate", summary.error_rate, 0.05).await?;
            }

            if summary.average_time_ms > 100.0 { // 100ms average threshold
                self.handle_performance_threshold_exceeded("average_time_ms", summary.average_time_ms, 100.0).await?;
            }

            if summary.cache_hit_rate < 0.8 { // 80% cache hit rate threshold
                self.handle_performance_threshold_exceeded("cache_hit_rate", summary.cache_hit_rate, 0.8).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}