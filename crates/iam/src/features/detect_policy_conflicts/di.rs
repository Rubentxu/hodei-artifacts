// crates/iam/src/features/detect_policy_conflicts/di.rs

use crate::infrastructure::errors::IamError;
use super::adapter::{
    CedarDirectConflictDetector, SimpleRedundancyDetector, SimpleUnreachableDetector,
    SimpleOverlapAnalyzer, SimpleConflictMetricsCollector, SimpleResolutionSuggester,
    DefaultConflictAnalysisConfigProvider,
};
use super::api::DetectPolicyConflictsApi;
use super::ports::{
    PolicyConflictDetectionService, DirectConflictDetector, PolicyRedundancyDetector,
    UnreachablePolicyDetector, PolicyOverlapAnalyzer, ConflictAnalysisMetricsCollector,
    ConflictResolutionSuggester, ConflictAnalysisConfigProvider,
};
use super::use_case::DetectPolicyConflictsUseCase;
use axum::Router;
use std::sync::Arc;

/// Dependency injection container for detect_policy_conflicts feature
pub struct DetectPolicyConflictsContainer {
    pub conflict_detection_service: Arc<dyn PolicyConflictDetectionService>,
    pub direct_conflict_detector: Arc<dyn DirectConflictDetector>,
    pub redundancy_detector: Arc<dyn PolicyRedundancyDetector>,
    pub unreachable_detector: Arc<dyn UnreachablePolicyDetector>,
    pub overlap_analyzer: Arc<dyn PolicyOverlapAnalyzer>,
    pub metrics_collector: Arc<dyn ConflictAnalysisMetricsCollector>,
    pub resolution_suggester: Arc<dyn ConflictResolutionSuggester>,
    pub config_provider: Arc<dyn ConflictAnalysisConfigProvider>,
}

impl DetectPolicyConflictsContainer {
    /// Create a new container with default implementations
    pub fn new() -> Result<Self, IamError> {
        let direct_conflict_detector = Arc::new(CedarDirectConflictDetector::new()?);
        let redundancy_detector = Arc::new(SimpleRedundancyDetector::new()?);
        let unreachable_detector = Arc::new(SimpleUnreachableDetector::new());
        let overlap_analyzer = Arc::new(SimpleOverlapAnalyzer::new());
        let metrics_collector = Arc::new(SimpleConflictMetricsCollector::new());
        let resolution_suggester = Arc::new(SimpleResolutionSuggester::new());
        let config_provider = Arc::new(DefaultConflictAnalysisConfigProvider::new());

        let conflict_detection_service = Arc::new(DetectPolicyConflictsUseCase::new(
            direct_conflict_detector.clone(),
            redundancy_detector.clone(),
            unreachable_detector.clone(),
            overlap_analyzer.clone(),
            metrics_collector.clone(),
            resolution_suggester.clone(),
            config_provider.clone(),
        ));

        Ok(Self {
            conflict_detection_service,
            direct_conflict_detector,
            redundancy_detector,
            unreachable_detector,
            overlap_analyzer,
            metrics_collector,
            resolution_suggester,
            config_provider,
        })
    }

    /// Create a new container with custom implementations
    pub fn with_custom_implementations(
        direct_conflict_detector: Arc<dyn DirectConflictDetector>,
        redundancy_detector: Arc<dyn PolicyRedundancyDetector>,
        unreachable_detector: Arc<dyn UnreachablePolicyDetector>,
        overlap_analyzer: Arc<dyn PolicyOverlapAnalyzer>,
        metrics_collector: Arc<dyn ConflictAnalysisMetricsCollector>,
        resolution_suggester: Arc<dyn ConflictResolutionSuggester>,
        config_provider: Arc<dyn ConflictAnalysisConfigProvider>,
    ) -> Self {
        let conflict_detection_service = Arc::new(DetectPolicyConflictsUseCase::new(
            direct_conflict_detector.clone(),
            redundancy_detector.clone(),
            unreachable_detector.clone(),
            overlap_analyzer.clone(),
            metrics_collector.clone(),
            resolution_suggester.clone(),
            config_provider.clone(),
        ));

        Self {
            conflict_detection_service,
            direct_conflict_detector,
            redundancy_detector,
            unreachable_detector,
            overlap_analyzer,
            metrics_collector,
            resolution_suggester,
            config_provider,
        }
    }

    /// Get the conflict detection service
    pub fn conflict_detection_service(&self) -> Arc<dyn PolicyConflictDetectionService> {
        self.conflict_detection_service.clone()
    }

    /// Create API router for this feature
    pub fn create_router(&self) -> Router {
        DetectPolicyConflictsApi::router(self.conflict_detection_service.clone())
    }

    /// Create API instance
    pub fn create_api(&self) -> DetectPolicyConflictsApi {
        DetectPolicyConflictsApi::new(self.conflict_detection_service.clone())
    }
}

impl Default for DetectPolicyConflictsContainer {
    fn default() -> Self {
        Self::new().expect("Failed to create default DetectPolicyConflictsContainer")
    }
}

/// Builder for DetectPolicyConflictsContainer
pub struct DetectPolicyConflictsContainerBuilder {
    direct_conflict_detector: Option<Arc<dyn DirectConflictDetector>>,
    redundancy_detector: Option<Arc<dyn PolicyRedundancyDetector>>,
    unreachable_detector: Option<Arc<dyn UnreachablePolicyDetector>>,
    overlap_analyzer: Option<Arc<dyn PolicyOverlapAnalyzer>>,
    metrics_collector: Option<Arc<dyn ConflictAnalysisMetricsCollector>>,
    resolution_suggester: Option<Arc<dyn ConflictResolutionSuggester>>,
    config_provider: Option<Arc<dyn ConflictAnalysisConfigProvider>>,
}

impl DetectPolicyConflictsContainerBuilder {
    pub fn new() -> Self {
        Self {
            direct_conflict_detector: None,
            redundancy_detector: None,
            unreachable_detector: None,
            overlap_analyzer: None,
            metrics_collector: None,
            resolution_suggester: None,
            config_provider: None,
        }
    }

    pub fn with_direct_conflict_detector(mut self, detector: Arc<dyn DirectConflictDetector>) -> Self {
        self.direct_conflict_detector = Some(detector);
        self
    }

    pub fn with_redundancy_detector(mut self, detector: Arc<dyn PolicyRedundancyDetector>) -> Self {
        self.redundancy_detector = Some(detector);
        self
    }

    pub fn with_unreachable_detector(mut self, detector: Arc<dyn UnreachablePolicyDetector>) -> Self {
        self.unreachable_detector = Some(detector);
        self
    }

    pub fn with_overlap_analyzer(mut self, analyzer: Arc<dyn PolicyOverlapAnalyzer>) -> Self {
        self.overlap_analyzer = Some(analyzer);
        self
    }

    pub fn with_metrics_collector(mut self, collector: Arc<dyn ConflictAnalysisMetricsCollector>) -> Self {
        self.metrics_collector = Some(collector);
        self
    }

    pub fn with_resolution_suggester(mut self, suggester: Arc<dyn ConflictResolutionSuggester>) -> Self {
        self.resolution_suggester = Some(suggester);
        self
    }

    pub fn with_config_provider(mut self, provider: Arc<dyn ConflictAnalysisConfigProvider>) -> Self {
        self.config_provider = Some(provider);
        self
    }

    pub fn build(self) -> Result<DetectPolicyConflictsContainer, IamError> {
        // Use defaults for missing components
        let direct_conflict_detector = self.direct_conflict_detector
            .unwrap_or_else(|| Arc::new(CedarDirectConflictDetector::new().unwrap()));
        let redundancy_detector = self.redundancy_detector
            .unwrap_or_else(|| Arc::new(SimpleRedundancyDetector::new().unwrap()));
        let unreachable_detector = self.unreachable_detector
            .unwrap_or_else(|| Arc::new(SimpleUnreachableDetector::new()));
        let overlap_analyzer = self.overlap_analyzer
            .unwrap_or_else(|| Arc::new(SimpleOverlapAnalyzer::new()));
        let metrics_collector = self.metrics_collector
            .unwrap_or_else(|| Arc::new(SimpleConflictMetricsCollector::new()));
        let resolution_suggester = self.resolution_suggester
            .unwrap_or_else(|| Arc::new(SimpleResolutionSuggester::new()));
        let config_provider = self.config_provider
            .unwrap_or_else(|| Arc::new(DefaultConflictAnalysisConfigProvider::new()));

        Ok(DetectPolicyConflictsContainer::with_custom_implementations(
            direct_conflict_detector,
            redundancy_detector,
            unreachable_detector,
            overlap_analyzer,
            metrics_collector,
            resolution_suggester,
            config_provider,
        ))
    }
}

impl Default for DetectPolicyConflictsContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory for creating specialized conflict detection containers
pub struct ConflictDetectionContainerFactory;

impl ConflictDetectionContainerFactory {
    /// Create a container optimized for fast conflict detection
    pub fn create_fast_detection_container() -> Result<DetectPolicyConflictsContainer, IamError> {
        DetectPolicyConflictsContainerBuilder::new()
            // Use default implementations which are optimized for speed
            .build()
    }

    /// Create a container optimized for comprehensive analysis
    pub fn create_comprehensive_analysis_container() -> Result<DetectPolicyConflictsContainer, IamError> {
        DetectPolicyConflictsContainerBuilder::new()
            // All analysis types enabled by default
            .build()
    }

    /// Create a container for testing with mock implementations
    #[cfg(test)]
    pub fn create_test_container() -> Result<DetectPolicyConflictsContainer, IamError> {
        use crate::features::detect_policy_conflicts::di::tests::mocks::*;
        
        DetectPolicyConflictsContainerBuilder::new()
            .with_direct_conflict_detector(Arc::new(MockDirectConflictDetector::new()))
            .with_redundancy_detector(Arc::new(MockRedundancyDetector::new()))
            .with_unreachable_detector(Arc::new(MockUnreachableDetector::new()))
            .with_overlap_analyzer(Arc::new(MockOverlapAnalyzer::new()))
            .with_metrics_collector(Arc::new(MockMetricsCollector::new()))
            .with_resolution_suggester(Arc::new(MockResolutionSuggester::new()))
            .with_config_provider(Arc::new(MockConfigProvider::new()))
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_creation() {
        let container = DetectPolicyConflictsContainer::new();
        assert!(container.is_ok());
    }

    #[test]
    fn test_container_builder() {
        let container = DetectPolicyConflictsContainerBuilder::new().build();
        assert!(container.is_ok());
    }

    #[test]
    fn test_container_router_creation() {
        let container = DetectPolicyConflictsContainer::new().unwrap();
        let _router = container.create_router();
        // Router creation should not panic
    }

    #[test]
    fn test_container_api_creation() {
        let container = DetectPolicyConflictsContainer::new().unwrap();
        let _api = container.create_api();
        // API creation should not panic
    }

    #[test]
    fn test_factory_fast_detection_container() {
        let container = ConflictDetectionContainerFactory::create_fast_detection_container();
        assert!(container.is_ok());
    }

    #[test]
    fn test_factory_comprehensive_analysis_container() {
        let container = ConflictDetectionContainerFactory::create_comprehensive_analysis_container();
        assert!(container.is_ok());
    }

    #[test]
    fn test_factory_test_container() {
        let container = ConflictDetectionContainerFactory::create_test_container();
        assert!(container.is_ok());
    }

    #[test]
    fn test_builder_with_custom_components() {
        let direct_detector = Arc::new(CedarDirectConflictDetector::new().unwrap());
        let redundancy_detector = Arc::new(SimpleRedundancyDetector::new().unwrap());
        
        let container = DetectPolicyConflictsContainerBuilder::new()
            .with_direct_conflict_detector(direct_detector)
            .with_redundancy_detector(redundancy_detector)
            .build();
        
        assert!(container.is_ok());
    }

    // Mock implementations for testing
    pub mod mocks {
        use crate::features::detect_policy_conflicts::dto::*;
        use crate::features::detect_policy_conflicts::ports::*;
        use crate::infrastructure::errors::IamError;
        use async_trait::async_trait;

        pub struct MockDirectConflictDetector;
        impl MockDirectConflictDetector {
            pub fn new() -> Self { Self }
        }

        #[async_trait]
        impl DirectConflictDetector for MockDirectConflictDetector {
            async fn detect_direct_conflicts(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
                Ok(vec![])
            }

            async fn check_policy_pair_conflict(&self, _policy1: &PolicyForAnalysis, _policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
                Ok(None)
            }
        }

        pub struct MockRedundancyDetector;
        impl MockRedundancyDetector {
            pub fn new() -> Self { Self }
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

        pub struct MockUnreachableDetector;
        impl MockUnreachableDetector {
            pub fn new() -> Self { Self }
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

        pub struct MockOverlapAnalyzer;
        impl MockOverlapAnalyzer {
            pub fn new() -> Self { Self }
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

        pub struct MockMetricsCollector;
        impl MockMetricsCollector {
            pub fn new() -> Self { Self }
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

        pub struct MockResolutionSuggester;
        impl MockResolutionSuggester {
            pub fn new() -> Self { Self }
        }

        impl ConflictResolutionSuggester for MockResolutionSuggester {
            fn suggest_conflict_resolution(&self, _conflict: &PolicyConflict) -> Option<String> {
                Some("Mock resolution".to_string())
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

        pub struct MockConfigProvider;
        impl MockConfigProvider {
            pub fn new() -> Self { Self }
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
    }
}