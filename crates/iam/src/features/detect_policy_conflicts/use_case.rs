// crates/iam/src/features/detect_policy_conflicts/use_case.rs

use crate::infrastructure::errors::IamError;
use super::dto::*;
use super::ports::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// Use case for comprehensive policy conflict detection
pub struct DetectPolicyConflictsUseCase {
    direct_conflict_detector: Arc<dyn DirectConflictDetector>,
    redundancy_detector: Arc<dyn PolicyRedundancyDetector>,
    unreachable_detector: Arc<dyn UnreachablePolicyDetector>,
    overlap_analyzer: Arc<dyn PolicyOverlapAnalyzer>,
    metrics_collector: Arc<dyn ConflictAnalysisMetricsCollector>,
    resolution_suggester: Arc<dyn ConflictResolutionSuggester>,
    config_provider: Arc<dyn ConflictAnalysisConfigProvider>,
}

impl DetectPolicyConflictsUseCase {
    pub fn new(
        direct_conflict_detector: Arc<dyn DirectConflictDetector>,
        redundancy_detector: Arc<dyn PolicyRedundancyDetector>,
        unreachable_detector: Arc<dyn UnreachablePolicyDetector>,
        overlap_analyzer: Arc<dyn PolicyOverlapAnalyzer>,
        metrics_collector: Arc<dyn ConflictAnalysisMetricsCollector>,
        resolution_suggester: Arc<dyn ConflictResolutionSuggester>,
        config_provider: Arc<dyn ConflictAnalysisConfigProvider>,
    ) -> Self {
        Self {
            direct_conflict_detector,
            redundancy_detector,
            unreachable_detector,
            overlap_analyzer,
            metrics_collector,
            resolution_suggester,
            config_provider,
        }
    }

    /// Execute the conflict detection use case
    pub async fn execute(&self, request: DetectPolicyConflictsRequest) -> Result<DetectPolicyConflictsResponse, IamError> {
        let operation_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        // Validate input
        if request.policies.is_empty() {
            return Err(IamError::validation_error("At least one policy is required for conflict analysis"));
        }

        if request.policies.len() > 1000 {
            return Err(IamError::validation_error("Too many policies for analysis (maximum 1000)"));
        }

        // Start metrics collection
        self.metrics_collector.start_analysis_metrics(&operation_id).await?;

        // Get analysis options (use defaults if not provided)
        let options = request.options.unwrap_or_else(|| self.config_provider.get_default_options());

        // Check timeout
        let timeout_ms = options.timeout_ms.unwrap_or_else(|| self.config_provider.get_analysis_timeout());
        
        // Perform conflict analysis
        let conflict_analysis = self.perform_conflict_analysis(&request.policies, &options, &operation_id).await?;
        
        // Collect final metrics
        let mut metrics = self.metrics_collector.finish_analysis_metrics(&operation_id).await?;
        metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;

        // Check performance thresholds
        self.check_performance_thresholds(&metrics)?;

        // Create response
        let response = DetectPolicyConflictsResponse::with_conflicts(conflict_analysis, metrics);

        Ok(response)
    }

    async fn perform_conflict_analysis(
        &self,
        policies: &[PolicyForAnalysis],
        options: &ConflictAnalysisOptions,
        operation_id: &str,
    ) -> Result<PolicyConflictAnalysis, IamError> {
        let mut conflicts = Vec::new();
        let mut redundancies = Vec::new();
        let mut unreachable_policies = Vec::new();

        // Step 1: Detect direct conflicts
        let direct_conflicts_start = Instant::now();
        let direct_conflicts = self.direct_conflict_detector.detect_direct_conflicts(policies).await?;
        let direct_conflicts_duration = direct_conflicts_start.elapsed().as_millis() as u64;
        self.metrics_collector.record_analysis_step(operation_id, "direct_conflicts", direct_conflicts_duration).await?;
        
        conflicts.extend(direct_conflicts);

        // Step 2: Detect permission overlaps
        let overlaps_start = Instant::now();
        let overlap_conflicts = self.overlap_analyzer.analyze_permission_overlaps(policies).await?;
        let overlaps_duration = overlaps_start.elapsed().as_millis() as u64;
        self.metrics_collector.record_analysis_step(operation_id, "permission_overlaps", overlaps_duration).await?;
        
        conflicts.extend(overlap_conflicts);

        // Step 3: Detect redundancies (if enabled)
        if options.detect_redundancies.unwrap_or(true) {
            let redundancy_start = Instant::now();
            redundancies = self.redundancy_detector.detect_redundancies(policies).await?;
            let redundancy_duration = redundancy_start.elapsed().as_millis() as u64;
            self.metrics_collector.record_analysis_step(operation_id, "redundancy_analysis", redundancy_duration).await?;
        }

        // Step 4: Detect unreachable policies (if enabled)
        if options.find_unreachable.unwrap_or(true) {
            let unreachable_start = Instant::now();
            unreachable_policies = self.unreachable_detector.detect_unreachable_policies(policies).await?;
            let unreachable_duration = unreachable_start.elapsed().as_millis() as u64;
            self.metrics_collector.record_analysis_step(operation_id, "reachability_analysis", unreachable_duration).await?;
        }

        // Step 5: Enhance conflicts with resolution suggestions
        if options.include_explanations.unwrap_or(true) {
            for conflict in &mut conflicts {
                if conflict.suggested_resolution.is_none() {
                    conflict.suggested_resolution = self.resolution_suggester.suggest_conflict_resolution(conflict);
                }
            }

            for redundancy in &mut redundancies {
                if redundancy.explanation.is_empty() {
                    redundancy.explanation = self.resolution_suggester.explain_redundancy(redundancy);
                }
            }

            for unreachable in &mut unreachable_policies {
                if unreachable.explanation.is_empty() {
                    unreachable.explanation = self.resolution_suggester.explain_unreachability(unreachable);
                }
            }
        }

        // Step 6: Calculate combinations analyzed
        let total_combinations = self.calculate_combinations_analyzed(policies.len());
        self.metrics_collector.record_combinations_analyzed(operation_id, total_combinations).await?;

        // Step 7: Generate summary
        let summary = self.generate_conflict_summary(policies, &conflicts, &redundancies, &unreachable_policies);

        Ok(PolicyConflictAnalysis {
            conflicts,
            redundancies,
            unreachable_policies,
            summary,
        })
    }

    fn calculate_combinations_analyzed(&self, policy_count: usize) -> u64 {
        // For n policies, we analyze n*(n-1)/2 pairs for direct conflicts
        // Plus additional combinations for overlap analysis
        let pair_combinations = (policy_count * (policy_count - 1)) / 2;
        let overlap_combinations = policy_count * policy_count; // Simplified
        (pair_combinations + overlap_combinations) as u64
    }

    fn generate_conflict_summary(
        &self,
        policies: &[PolicyForAnalysis],
        conflicts: &[PolicyConflict],
        redundancies: &[PolicyRedundancy],
        unreachable_policies: &[UnreachablePolicy],
    ) -> ConflictSummary {
        let total_issues = conflicts.len() + redundancies.len() + unreachable_policies.len();
        let conflict_score = if policies.is_empty() {
            0.0
        } else {
            (total_issues as f64) / (policies.len() as f64)
        };

        ConflictSummary {
            total_policies: policies.len(),
            total_conflicts: conflicts.len(),
            total_redundancies: redundancies.len(),
            total_unreachable: unreachable_policies.len(),
            conflict_score: conflict_score.min(1.0), // Cap at 1.0
        }
    }

    fn check_performance_thresholds(&self, metrics: &ConflictAnalysisMetrics) -> Result<(), IamError> {
        let thresholds = self.config_provider.get_performance_thresholds();

        if metrics.total_duration_ms > thresholds.max_analysis_time_ms {
            return Err(IamError::validation_error(format!(
                "Conflict analysis exceeded maximum time threshold: {}ms > {}ms",
                metrics.total_duration_ms, thresholds.max_analysis_time_ms
            )));
        }

        if let Some(memory_usage) = metrics.memory_usage_bytes {
            if memory_usage > thresholds.max_memory_usage_bytes {
                return Err(IamError::validation_error(format!(
                    "Conflict analysis exceeded maximum memory threshold: {} bytes > {} bytes",
                    memory_usage, thresholds.max_memory_usage_bytes
                )));
            }
        }

        if metrics.combinations_analyzed > thresholds.max_combinations {
            return Err(IamError::validation_error(format!(
                "Conflict analysis exceeded maximum combinations threshold: {} > {}",
                metrics.combinations_analyzed, thresholds.max_combinations
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl PolicyConflictDetectionService for DetectPolicyConflictsUseCase {
    async fn detect_conflicts(&self, request: DetectPolicyConflictsRequest) -> Result<DetectPolicyConflictsResponse, IamError> {
        self.execute(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Mock implementations for testing
    struct MockDirectConflictDetector;
    struct MockRedundancyDetector;
    struct MockUnreachableDetector;
    struct MockOverlapAnalyzer;
    struct MockMetricsCollector;
    struct MockResolutionSuggester;
    struct MockConfigProvider;

    #[async_trait]
    impl DirectConflictDetector for MockDirectConflictDetector {
        async fn detect_direct_conflicts(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
            Ok(vec![])
        }

        async fn check_policy_pair_conflict(&self, _policy1: &PolicyForAnalysis, _policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl PolicyRedundancyDetector for MockRedundancyDetector {
        async fn detect_redundancies(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyRedundancy>, IamError> {
            Ok(vec![])
        }

        async fn is_policy_redundant(&self, _target_policy: &PolicyForAnalysis, _other_policies: &[PolicyForAnalysis]) -> Result<Option<PolicyRedundancy>, IamError> {
            Ok(None)
        }

        async fn calculate_redundancy_confidence(&self, _redundant_policy: &PolicyForAnalysis, _superseding_policies: &[PolicyForAnalysis]) -> Result<f64, IamError> {
            Ok(0.0)
        }
    }

    #[async_trait]
    impl UnreachablePolicyDetector for MockUnreachableDetector {
        async fn detect_unreachable_policies(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<UnreachablePolicy>, IamError> {
            Ok(vec![])
        }

        async fn is_policy_unreachable(&self, _target_policy: &PolicyForAnalysis, _other_policies: &[PolicyForAnalysis]) -> Result<Option<UnreachablePolicy>, IamError> {
            Ok(None)
        }

        async fn find_reachability_conditions(&self, _policy: &PolicyForAnalysis, _blocking_policies: &[PolicyForAnalysis]) -> Result<Option<String>, IamError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl PolicyOverlapAnalyzer for MockOverlapAnalyzer {
        async fn analyze_permission_overlaps(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
            Ok(vec![])
        }

        async fn calculate_overlap_score(&self, _policy1: &PolicyForAnalysis, _policy2: &PolicyForAnalysis) -> Result<f64, IamError> {
            Ok(0.0)
        }

        async fn find_common_patterns(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<String>, IamError> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl ConflictAnalysisMetricsCollector for MockMetricsCollector {
        async fn start_analysis_metrics(&self, _operation_id: &str) -> Result<(), IamError> {
            Ok(())
        }

        async fn record_analysis_step(&self, _operation_id: &str, _step_name: &str, _duration_ms: u64) -> Result<(), IamError> {
            Ok(())
        }

        async fn record_combinations_analyzed(&self, _operation_id: &str, _count: u64) -> Result<(), IamError> {
            Ok(())
        }

        async fn finish_analysis_metrics(&self, _operation_id: &str) -> Result<ConflictAnalysisMetrics, IamError> {
            Ok(ConflictAnalysisMetrics::default())
        }
    }

    impl ConflictResolutionSuggester for MockResolutionSuggester {
        fn suggest_conflict_resolution(&self, _conflict: &PolicyConflict) -> Option<String> {
            Some("Mock resolution suggestion".to_string())
        }

        fn explain_redundancy(&self, _redundancy: &PolicyRedundancy) -> String {
            "Mock redundancy explanation".to_string()
        }

        fn explain_unreachability(&self, _unreachable: &UnreachablePolicy) -> String {
            "Mock unreachability explanation".to_string()
        }

        fn suggest_priority_adjustments(&self, _conflicts: &[PolicyConflict]) -> Vec<PriorityAdjustment> {
            vec![]
        }
    }

    impl ConflictAnalysisConfigProvider for MockConfigProvider {
        fn get_default_options(&self) -> ConflictAnalysisOptions {
            ConflictAnalysisOptions::default()
        }

        fn get_analysis_timeout(&self) -> u64 {
            30000
        }

        fn is_analysis_enabled(&self, _analysis_type: AnalysisType) -> bool {
            true
        }

        fn get_performance_thresholds(&self) -> AnalysisPerformanceThresholds {
            AnalysisPerformanceThresholds::default()
        }

        fn get_redundancy_threshold(&self) -> f64 {
            0.8
        }
    }

    #[tokio::test]
    async fn test_detect_policy_conflicts_use_case_success() {
        let use_case = DetectPolicyConflictsUseCase::new(
            Arc::new(MockDirectConflictDetector),
            Arc::new(MockRedundancyDetector),
            Arc::new(MockUnreachableDetector),
            Arc::new(MockOverlapAnalyzer),
            Arc::new(MockMetricsCollector),
            Arc::new(MockResolutionSuggester),
            Arc::new(MockConfigProvider),
        );

        let policy1 = PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string());
        let policy2 = PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string());
        let request = DetectPolicyConflictsRequest::new(vec![policy1, policy2]);

        let result = use_case.execute(request).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.conflict_analysis.summary.total_policies, 2);
    }

    #[tokio::test]
    async fn test_detect_policy_conflicts_empty_policies() {
        let use_case = DetectPolicyConflictsUseCase::new(
            Arc::new(MockDirectConflictDetector),
            Arc::new(MockRedundancyDetector),
            Arc::new(MockUnreachableDetector),
            Arc::new(MockOverlapAnalyzer),
            Arc::new(MockMetricsCollector),
            Arc::new(MockResolutionSuggester),
            Arc::new(MockConfigProvider),
        );

        let request = DetectPolicyConflictsRequest::new(vec![]);
        let result = use_case.execute(request).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("At least one policy is required"));
    }

    #[tokio::test]
    async fn test_detect_policy_conflicts_too_many_policies() {
        let use_case = DetectPolicyConflictsUseCase::new(
            Arc::new(MockDirectConflictDetector),
            Arc::new(MockRedundancyDetector),
            Arc::new(MockUnreachableDetector),
            Arc::new(MockOverlapAnalyzer),
            Arc::new(MockMetricsCollector),
            Arc::new(MockResolutionSuggester),
            Arc::new(MockConfigProvider),
        );

        // Create more than 1000 policies
        let policies: Vec<PolicyForAnalysis> = (0..1001)
            .map(|i| PolicyForAnalysis::new(format!("policy{}", i), "permit(principal, action, resource);".to_string()))
            .collect();

        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many policies"));
    }

    #[test]
    fn test_calculate_combinations_analyzed() {
        let use_case = DetectPolicyConflictsUseCase::new(
            Arc::new(MockDirectConflictDetector),
            Arc::new(MockRedundancyDetector),
            Arc::new(MockUnreachableDetector),
            Arc::new(MockOverlapAnalyzer),
            Arc::new(MockMetricsCollector),
            Arc::new(MockResolutionSuggester),
            Arc::new(MockConfigProvider),
        );

        // For 3 policies: pairs = 3*2/2 = 3, overlaps = 3*3 = 9, total = 12
        let combinations = use_case.calculate_combinations_analyzed(3);
        assert_eq!(combinations, 12);

        // For 4 policies: pairs = 4*3/2 = 6, overlaps = 4*4 = 16, total = 22
        let combinations = use_case.calculate_combinations_analyzed(4);
        assert_eq!(combinations, 22);
    }
}