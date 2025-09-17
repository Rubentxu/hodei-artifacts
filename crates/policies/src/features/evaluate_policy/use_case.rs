//! Core use case for policy evaluation
//! 
//! This module implements the main business logic for policy evaluation.

use super::dto::*;
use super::error::EvaluatePolicyError;
use crate::domain::context::PolicyEvaluationContext;
use crate::domain::decision::PolicyDecision;
use crate::features::evaluate_policy::ports::*;
use async_trait::async_trait;
use shared::hrn::PolicyId;
use std::sync::Arc;
use tokio::time::Instant;
use tracing::{debug, error, info, span, Instrument, Level};


/// Main use case for policy evaluation
pub struct EvaluatePolicyUseCase {
    policy_storage: Arc<dyn PolicyStoragePort>,
    cedar_engine: Arc<dyn CedarEnginePort>,
    policy_cache: Arc<dyn PolicyCachePort>,
    performance_monitor: Arc<dyn PerformanceMonitorPort>,
    event_publisher: Arc<dyn EventPublisherPort>,
    cache_ttl_seconds: u64,
}

impl EvaluatePolicyUseCase {
    /// Create a new policy evaluation use case
    pub fn new(
        policy_storage: Arc<dyn PolicyStoragePort>,
        cedar_engine: Arc<dyn CedarEnginePort>,
        policy_cache: Arc<dyn PolicyCachePort>,
        performance_monitor: Arc<dyn PerformanceMonitorPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
        cache_ttl_seconds: u64,
    ) -> Self {
        Self {
            policy_storage,
            cedar_engine,
            policy_cache,
            performance_monitor,
            event_publisher,
            cache_ttl_seconds,
        }
    }

    /// Evaluate a single policy against a context
    pub async fn evaluate_policy(
        &self,
        request: EvaluatePolicyRequest,
    ) -> Result<EvaluatePolicyResponse, EvaluatePolicyError> {
        let span = span!(Level::INFO, "evaluate_policy", policy_id = %request.policy_id);
        
        async move {
            let start_time = Instant::now();
            
            debug!(
                policy_id = %request.policy_id,
                principal = %request.context.principal,
                action = %request.context.action,
                resource = %request.context.resource.resource_id,
                "Starting policy evaluation"
            );

            // Check cache first
            let cache_key = self.calculate_cache_key(&request.policy_id, &request.context);
            if let Some(cached_result) = self.policy_cache.get_cached_result(&cache_key).await? {
                debug!("Cache hit for policy evaluation");
                
                let response = self.build_cached_response(request.policy_id, cached_result, start_time).await?;
                
                // Record cache hit metrics
                self.record_cache_metrics(&response.metrics, true).await?;
                
                return Ok(response);
            }

            debug!("Cache miss, proceeding with full evaluation");

            // Get the policy
            let policy = self.policy_storage.get_policy(&request.policy_id).await?
                .ok_or_else(|| EvaluatePolicyError::PolicyNotFound(request.policy_id.clone()))?;

            // Validate policy syntax
            self.cedar_engine.validate_policy_syntax(&policy.content).await?;

            // Compile the policy
            let compiled_policy = self.cedar_engine.compile_policy(&policy.content).await?;

            // Evaluate the policy
            let decision = self.cedar_engine.evaluate_policy(&*compiled_policy, &request.context).await?;

            // Cache the result
            let cached_result = CachedEvaluationResult {
                decision: decision.clone(),
                cached_at: chrono::Utc::now(),
                ttl_seconds: self.cache_ttl_seconds,
            };
            self.policy_cache.cache_result(&cache_key, &cached_result, self.cache_ttl_seconds).await?;

            // Build response
            let response = self.build_response(request.policy_id, decision, start_time).await?;

            // Record evaluation metrics
            self.record_evaluation_metrics(&response.metrics).await?;

            // Publish events
            self.publish_evaluation_events(&request, &response).await?;

            info!(
                policy_id = %request.policy_id,
                decision_allowed = %response.decision.allowed,
                evaluation_time_ms = %response.metrics.evaluation_time_ms,
                cache_hit_rate = %response.metrics.cache_hit_rate,
                "Policy evaluation completed successfully"
            );

            Ok(response)
        }
        .instrument(span)
        .await
    }

    /// Batch evaluate multiple policies
    pub async fn batch_evaluate_policies(
        &self,
        request: BatchEvaluatePolicyRequest,
    ) -> Result<BatchEvaluatePolicyResponse, EvaluatePolicyError> {
        let span = span!(Level::INFO, "batch_evaluate_policies", count = %request.requests.len());
        
        async move {
            let start_time = Instant::now();
            let mut responses = Vec::with_capacity(request.requests.len());
            let mut successful_count = 0;
            let mut failed_count = 0;

            if request.parallel {
                // Parallel evaluation
                let tasks: Vec<_> = request.requests
                    .into_iter()
                    .map(|req| self.evaluate_policy(req))
                    .collect();

                let results = futures::future::join_all(tasks).await;
                
                for result in results {
                    match result {
                        Ok(response) => {
                            responses.push(response);
                            successful_count += 1;
                        }
                        Err(e) => {
                            error!(error = %e, "Batch evaluation failed");
                            failed_count += 1;
                        }
                    }
                }
            } else {
                // Sequential evaluation
                for req in request.requests {
                    match self.evaluate_policy(req).await {
                        Ok(response) => {
                            responses.push(response);
                            successful_count += 1;
                        }
                        Err(e) => {
                            error!(error = %e, "Batch evaluation failed");
                            failed_count += 1;
                        }
                    }
                }
            }

            let total_time_ms = start_time.elapsed().as_millis() as u64;
            let average_time_ms = if !responses.is_empty() {
                total_time_ms as f64 / responses.len() as f64
            } else {
                0.0
            };

            let batch_metrics = BatchEvaluationMetrics {
                total_time_ms,
                successful_evaluations: successful_count,
                failed_evaluations: failed_count,
                average_time_ms,
            };

            info!(
                total_time_ms = %batch_metrics.total_time_ms,
                successful = %batch_metrics.successful_evaluations,
                failed = %batch_metrics.failed_evaluations,
                average_time_ms = %batch_metrics.average_time_ms,
                "Batch policy evaluation completed"
            );

            Ok(BatchEvaluatePolicyResponse {
                responses,
                batch_metrics,
            })
        }
        .instrument(span)
        .await
    }

    /// Find policies applicable to a context
    pub async fn find_applicable_policies(
        &self,
        query: FindApplicablePoliciesQuery,
    ) -> Result<FindApplicablePoliciesResponse, FindApplicablePoliciesError> {
        let span = span!(Level::INFO, "find_applicable_policies");
        
        async move {
            let start_time = Instant::now();
            
            // Create evaluation context for search
            let context = PolicyEvaluationContext {
                principal: query.principal,
                action: query.action,
                resource: ResourceContext {
                    resource_id: query.resource,
                    resource_type: "any".to_string(), // Would be derived from resource HRN
                    attributes: query.environment.unwrap_or_default(),
                },
                environment: query.environment.unwrap_or_default(),
                organization_id: query.organization_id.unwrap_or_default(),
            };

            // Find applicable policies
            let policies = self.policy_storage.find_applicable_policies(&context).await?;
            
            let policy_ids = policies.into_iter().map(|p| p.id).collect();
            let search_time_ms = start_time.elapsed().as_millis() as u64;

            let search_metrics = SearchMetrics {
                search_time_ms,
                policies_scanned: policy_ids.len() as u32,
                policies_matched: policy_ids.len() as u32,
            };

            debug!(
                found_policies = %policy_ids.len(),
                search_time_ms = %search_time_ms,
                "Found applicable policies"
            );

            Ok(FindApplicablePoliciesResponse {
                policy_ids,
                search_metrics,
            })
        }
        .instrument(span)
        .await
    }

    /// Get policy evaluation statistics
    pub async fn get_evaluation_statistics(&self) -> Result<EvaluationStatistics, EvaluatePolicyError> {
        let span = span!(Level::INFO, "get_evaluation_statistics");
        
        async move {
            let cache_stats = self.policy_cache.get_cache_stats().await?;
            let performance_summary = self.performance_monitor.get_performance_summary().await?;
            let engine_metrics = self.cedar_engine.get_engine_metrics().await?;

            let statistics = EvaluationStatistics {
                total_evaluations: performance_summary.total_evaluations,
                average_evaluation_time_ms: performance_summary.average_time_ms,
                success_rate: performance_summary.success_rate,
                cache_hit_rate: cache_stats.hit_rate,
                error_rate: performance_summary.error_rate,
                cache_total_entries: cache_stats.total_entries,
                cache_memory_usage_bytes: cache_stats.memory_usage_bytes,
                engine_policies_loaded: engine_metrics.policies_loaded,
                engine_compilation_cache_size: engine_metrics.compilation_cache_size,
                engine_uptime_seconds: engine_metrics.uptime_seconds,
                calculated_at: chrono::Utc::now(),
            };

            debug!(
                total_evaluations = %statistics.total_evaluations,
                cache_hit_rate = %statistics.cache_hit_rate,
                "Retrieved evaluation statistics"
            );

            Ok(statistics)
        }
        .instrument(span)
        .await
    }

    /// Validate policy syntax without evaluation
    pub async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<PolicyValidationResult, EvaluatePolicyError> {
        let span = span!(Level::INFO, "validate_policy");
        
        async move {
            let start_time = Instant::now();
            
            // Validate syntax
            self.cedar_engine.validate_policy_syntax(policy_content).await?;

            // Attempt compilation
            let compiled = self.cedar_engine.compile_policy(policy_content).await?;
            
            let validation_time_ms = start_time.elapsed().as_millis() as u64;

            let result = PolicyValidationResult {
                is_valid: true,
                validation_time_ms,
                compilation_hash: compiled.hash().to_string(),
                compiled_at: compiled.compiled_at(),
                errors: vec![],
                warnings: vec![],
            };

            debug!(
                is_valid = %result.is_valid,
                validation_time_ms = %validation_time_ms,
                "Policy validation completed successfully"
            );

            Ok(result)
        }
        .instrument(span)
        .await
    }

    // Helper methods

    fn calculate_cache_key(&self, policy_id: &PolicyId, context: &PolicyEvaluationContext) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        
        hasher.update(policy_id.to_string().as_bytes());
        hasher.update(context.principal.as_bytes());
        hasher.update(context.action.as_bytes());
        hasher.update(context.resource.resource_id.as_bytes());
        hasher.update(context.organization_id.as_bytes());
        
        // Add environment context to hash
        for (key, value) in &context.environment {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }
        
        format!("policy_eval:{:x}", hasher.finalize())
    }

    async fn build_response(
        &self,
        policy_id: PolicyId,
        decision: PolicyDecision,
        start_time: Instant,
    ) -> Result<EvaluatePolicyResponse, EvaluatePolicyError> {
        let evaluation_time_ms = start_time.elapsed().as_millis() as u64;
        
        Ok(EvaluatePolicyResponse {
            decision,
            policy_id,
            evaluated_at: chrono::Utc::now(),
            metrics: EvaluationMetrics {
                evaluation_time_ms,
                policies_evaluated: 1,
                cache_hit_rate: 0.0, // Cache miss
                memory_usage_bytes: 0, // Would be measured
            },
        })
    }

    async fn build_cached_response(
        &self,
        policy_id: PolicyId,
        cached_result: CachedEvaluationResult,
        start_time: Instant,
    ) -> Result<EvaluatePolicyResponse, EvaluatePolicyError> {
        let evaluation_time_ms = start_time.elapsed().as_millis() as u64;
        
        Ok(EvaluatePolicyResponse {
            decision: cached_result.decision,
            policy_id,
            evaluated_at: chrono::Utc::now(),
            metrics: EvaluationMetrics {
                evaluation_time_ms,
                policies_evaluated: 1,
                cache_hit_rate: 1.0, // Cache hit
                memory_usage_bytes: 0, // Would be measured
            },
        })
    }

    async fn record_evaluation_metrics(&self, metrics: &EvaluationMetrics) -> Result<(), EvaluatePolicyError> {
        self.performance_monitor.record_evaluation(metrics.clone()).await
    }

    async fn record_cache_metrics(&self, metrics: &EvaluationMetrics, is_hit: bool) -> Result<(), EvaluatePolicyError> {
        let adjusted_metrics = EvaluationMetrics {
            cache_hit_rate: if is_hit { 1.0 } else { 0.0 },
            ..metrics.clone()
        };
        self.performance_monitor.record_evaluation(adjusted_metrics).await
    }

    async fn publish_evaluation_events(
        &self,
        request: &EvaluatePolicyRequest,
        response: &EvaluatePolicyResponse,
    ) -> Result<(), EvaluatePolicyError> {
        let evaluation_event = PolicyEvaluationEvent {
            policy_id: request.policy_id.clone(),
            context: request.context.clone(),
            decision: response.decision.clone(),
            evaluation_time_ms: response.metrics.evaluation_time_ms,
            timestamp: response.evaluated_at,
        };

        self.event_publisher.publish_evaluation_event(evaluation_event).await?;

        let decision_event = PolicyDecisionEvent {
            policy_id: request.policy_id.clone(),
            decision: response.decision.clone(),
            context_hash: self.calculate_cache_key(&request.policy_id, &request.context),
            timestamp: response.evaluated_at,
        };

        self.event_publisher.publish_decision_event(decision_event).await?;

        Ok(())
    }
}

// Type aliases for return types
pub type FindApplicablePoliciesError = EvaluatePolicyError;

/// Policy validation result
#[derive(Debug, Clone)]
pub struct PolicyValidationResult {
    pub is_valid: bool,
    pub validation_time_ms: u64,
    pub compilation_hash: String,
    pub compiled_at: chrono::DateTime<chrono::Utc>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Policy evaluation statistics
#[derive(Debug, Clone)]
pub struct EvaluationStatistics {
    pub total_evaluations: u64,
    pub average_evaluation_time_ms: f64,
    pub success_rate: f64,
    pub cache_hit_rate: f64,
    pub error_rate: f64,
    pub cache_total_entries: u32,
    pub cache_memory_usage_bytes: u64,
    pub engine_policies_loaded: u32,
    pub engine_compilation_cache_size: u32,
    pub engine_uptime_seconds: u64,
    pub calculated_at: chrono::DateTime<chrono::Utc>,
}