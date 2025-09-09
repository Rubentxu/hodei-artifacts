// crates/iam/src/features/detect_policy_conflicts/use_case_test.rs

#[cfg(test)]
mod tests {
    use super::super::dto::*;
    use super::super::ports::*;
    use super::super::use_case::DetectPolicyConflictsUseCase;
    use crate::infrastructure::errors::IamError;
    use async_trait::async_trait;
    use std::sync::Arc;

    // Mock implementations for testing
    struct MockDirectConflictDetector {
        should_find_conflicts: bool,
    }

    struct MockRedundancyDetector {
        should_find_redundancies: bool,
    }

    struct MockUnreachableDetector {
        should_find_unreachable: bool,
    }

    struct MockOverlapAnalyzer {
        should_find_overlaps: bool,
    }

    struct MockMetricsCollector {
        metrics: std::sync::Mutex<ConflictAnalysisMetrics>,
    }

    struct MockResolutionSuggester;

    struct MockConfigProvider;

    impl MockDirectConflictDetector {
        fn new(should_find_conflicts: bool) -> Self {
            Self { should_find_conflicts }
        }
    }

    #[async_trait]
    impl DirectConflictDetector for MockDirectConflictDetector {
        async fn detect_direct_conflicts(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
            if self.should_find_conflicts && policies.len() >= 2 {
                Ok(vec![PolicyConflict {
                    conflict_type: ConflictType::DirectContradiction,
                    involved_policies: vec![
                        PolicyReference::new(policies[0].id.clone()),
                        PolicyReference::new(policies[1].id.clone()),
                    ],
                    description: "Mock direct conflict".to_string(),
                    severity: ConflictSeverity::High,
                    suggested_resolution: None,
                    location: None,
                }])
            } else {
                Ok(vec![])
            }
        }

        async fn check_policy_pair_conflict(&self, _policy1: &PolicyForAnalysis, _policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
            Ok(None)
        }
    }

    impl MockRedundancyDetector {
        fn new(should_find_redundancies: bool) -> Self {
            Self { should_find_redundancies }
        }
    }

    #[async_trait]
    impl PolicyRedundancyDetector for MockRedundancyDetector {
        async fn detect_redundancies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyRedundancy>, IamError> {
            if self.should_find_redundancies && policies.len() >= 2 {
                Ok(vec![PolicyRedundancy {
                    redundant_policy: PolicyReference::new(policies[1].id.clone()),
                    superseding_policies: vec![PolicyReference::new(policies[0].id.clone())],
                    explanation: "Mock redundancy".to_string(),
                    confidence: 0.9,
                }])
            } else {
                Ok(vec![])
            }
        }

        async fn is_policy_redundant(&self, _target_policy: &PolicyForAnalysis, _other_policies: &[PolicyForAnalysis]) -> Result<Option<PolicyRedundancy>, IamError> {
            Ok(None)
        }

        async fn calculate_redundancy_confidence(&self, _redundant_policy: &PolicyForAnalysis, _superseding_policies: &[PolicyForAnalysis]) -> Result<f64, IamError> {
            Ok(0.9)
        }
    }

    impl MockUnreachableDetector {
        fn new(should_find_unreachable: bool) -> Self {
            Self { should_find_unreachable }
        }
    }

    #[async_trait]
    impl UnreachablePolicyDetector for MockUnreachableDetector {
        async fn detect_unreachable_policies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<UnreachablePolicy>, IamError> {
            if self.should_find_unreachable && policies.len() >= 2 {
                Ok(vec![UnreachablePolicy {
                    policy: PolicyReference::new(policies[1].id.clone()),
                    blocking_policies: vec![PolicyReference::new(policies[0].id.clone())],
                    explanation: "Mock unreachable policy".to_string(),
                    reachability_conditions: Some("Add more specific conditions".to_string()),
                }])
            } else {
                Ok(vec![])
            }
        }

        async fn is_policy_unreachable(&self, _target_policy: &PolicyForAnalysis, _other_policies: &[PolicyForAnalysis]) -> Result<Option<UnreachablePolicy>, IamError> {
            Ok(None)
        }

        async fn find_reachability_conditions(&self, _policy: &PolicyForAnalysis, _blocking_policies: &[PolicyForAnalysis]) -> Result<Option<String>, IamError> {
            Ok(Some("Mock reachability conditions".to_string()))
        }
    }

    impl MockOverlapAnalyzer {
        fn new(should_find_overlaps: bool) -> Self {
            Self { should_find_overlaps }
        }
    }

    #[async_trait]
    impl PolicyOverlapAnalyzer for MockOverlapAnalyzer {
        async fn analyze_permission_overlaps(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
            if self.should_find_overlaps && policies.len() >= 2 {
                Ok(vec![PolicyConflict {
                    conflict_type: ConflictType::OverlappingPermissions,
                    involved_policies: vec![
                        PolicyReference::new(policies[0].id.clone()),
                        PolicyReference::new(policies[1].id.clone()),
                    ],
                    description: "Mock overlapping permissions".to_string(),
                    severity: ConflictSeverity::Medium,
                    suggested_resolution: None,
                    location: None,
                }])
            } else {
                Ok(vec![])
            }
        }

        async fn calculate_overlap_score(&self, _policy1: &PolicyForAnalysis, _policy2: &PolicyForAnalysis) -> Result<f64, IamError> {
            Ok(0.7)
        }

        async fn find_common_patterns(&self, _policies: &[PolicyForAnalysis]) -> Result<Vec<String>, IamError> {
            Ok(vec!["common_pattern".to_string()])
        }
    }

    impl MockMetricsCollector {
        fn new() -> Self {
            Self {
                metrics: std::sync::Mutex::new(ConflictAnalysisMetrics::default()),
            }
        }
    }

    #[async_trait]
    impl ConflictAnalysisMetricsCollector for MockMetricsCollector {
        async fn start_analysis_metrics(&self, _operation_id: &str) -> Result<(), IamError> {
            Ok(())
        }

        async fn record_analysis_step(&self, _operation_id: &str, step_name: &str, duration_ms: u64) -> Result<(), IamError> {
            let mut metrics = self.metrics.lock().unwrap();
            match step_name {
                "direct_conflicts" => metrics.conflict_detection_ms = duration_ms,
                "redundancy_analysis" => metrics.redundancy_analysis_ms = duration_ms,
                "reachability_analysis" => metrics.reachability_analysis_ms = duration_ms,
                _ => {}
            }
            Ok(())
        }

        async fn record_combinations_analyzed(&self, _operation_id: &str, count: u64) -> Result<(), IamError> {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.combinations_analyzed = count;
            Ok(())
        }

        async fn finish_analysis_metrics(&self, _operation_id: &str) -> Result<ConflictAnalysisMetrics, IamError> {
            let metrics = self.metrics.lock().unwrap();
            Ok(metrics.clone())
        }
    }

    impl ConflictResolutionSuggester for MockResolutionSuggester {
        fn suggest_conflict_resolution(&self, conflict: &PolicyConflict) -> Option<String> {
            Some(format!("Mock resolution for {:?}", conflict.conflict_type))
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

    fn create_test_use_case(
        find_conflicts: bool,
        find_redundancies: bool,
        find_unreachable: bool,
        find_overlaps: bool,
    ) -> DetectPolicyConflictsUseCase {
        DetectPolicyConflictsUseCase::new(
            Arc::new(MockDirectConflictDetector::new(find_conflicts)),
            Arc::new(MockRedundancyDetector::new(find_redundancies)),
            Arc::new(MockUnreachableDetector::new(find_unreachable)),
            Arc::new(MockOverlapAnalyzer::new(find_overlaps)),
            Arc::new(MockMetricsCollector::new()),
            Arc::new(MockResolutionSuggester),
            Arc::new(MockConfigProvider),
        )
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_success_no_conflicts() {
        let use_case = create_test_use_case(false, false, false, false);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.has_conflicts);
        assert_eq!(response.conflict_analysis.summary.total_policies, 2);
        assert_eq!(response.conflict_analysis.summary.total_conflicts, 0);
        assert_eq!(response.conflict_analysis.summary.total_redundancies, 0);
        assert_eq!(response.conflict_analysis.summary.total_unreachable, 0);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_direct_conflicts() {
        let use_case = create_test_use_case(true, false, false, false);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.has_conflicts);
        assert_eq!(response.conflict_analysis.summary.total_conflicts, 1);
        assert_eq!(response.conflict_analysis.conflicts[0].conflict_type, ConflictType::DirectContradiction);
        assert_eq!(response.conflict_analysis.conflicts[0].severity, ConflictSeverity::High);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_redundancies() {
        let use_case = create_test_use_case(false, true, false, false);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.has_conflicts);
        assert_eq!(response.conflict_analysis.summary.total_redundancies, 1);
        assert_eq!(response.conflict_analysis.redundancies[0].confidence, 0.9);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_unreachable_policies() {
        let use_case = create_test_use_case(false, false, true, false);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "forbid(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource) when condition;".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.has_conflicts);
        assert_eq!(response.conflict_analysis.summary.total_unreachable, 1);
        assert!(response.conflict_analysis.unreachable_policies[0].reachability_conditions.is_some());
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_overlapping_permissions() {
        let use_case = create_test_use_case(false, false, false, true);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.has_conflicts);
        assert_eq!(response.conflict_analysis.summary.total_conflicts, 1);
        assert_eq!(response.conflict_analysis.conflicts[0].conflict_type, ConflictType::OverlappingPermissions);
        assert_eq!(response.conflict_analysis.conflicts[0].severity, ConflictSeverity::Medium);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_all_types() {
        let use_case = create_test_use_case(true, true, true, true);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy3".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.has_conflicts);
        
        // Should have conflicts from direct conflicts and overlaps
        assert_eq!(response.conflict_analysis.summary.total_conflicts, 2);
        assert_eq!(response.conflict_analysis.summary.total_redundancies, 1);
        assert_eq!(response.conflict_analysis.summary.total_unreachable, 1);
        
        // Check conflict score
        assert!(response.conflict_analysis.summary.conflict_score > 0.0);
        assert!(response.conflict_analysis.summary.conflict_score <= 1.0);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_empty_policies() {
        let use_case = create_test_use_case(false, false, false, false);
        
        let request = DetectPolicyConflictsRequest::new(vec![]);
        let result = use_case.execute(request).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("At least one policy is required"));
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_too_many_policies() {
        let use_case = create_test_use_case(false, false, false, false);
        
        // Create more than 1000 policies
        let policies: Vec<PolicyForAnalysis> = (0..1001)
            .map(|i| PolicyForAnalysis::new(format!("policy{}", i), "permit(principal, action, resource);".to_string()))
            .collect();
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many policies"));
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_with_custom_options() {
        let use_case = create_test_use_case(true, true, true, true);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let options = ConflictAnalysisOptions {
            detect_redundancies: Some(false), // Disable redundancy detection
            find_unreachable: Some(false),    // Disable unreachable detection
            redundancy_threshold: Some(0.9),
            include_explanations: Some(true),
            timeout_ms: Some(5000),
        };
        
        let request = DetectPolicyConflictsRequest::new(policies).with_options(options);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        
        // Should only have direct conflicts and overlaps, no redundancies or unreachable
        assert!(response.conflict_analysis.summary.total_conflicts > 0);
        assert_eq!(response.conflict_analysis.summary.total_redundancies, 0);
        assert_eq!(response.conflict_analysis.summary.total_unreachable, 0);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_metrics_collection() {
        let use_case = create_test_use_case(true, true, true, true);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        
        // Verify metrics are collected
        assert!(response.metrics.total_duration_ms > 0);
        assert!(response.metrics.combinations_analyzed > 0);
        
        // For 2 policies: pairs = 1, overlaps = 4, total = 5
        assert_eq!(response.metrics.combinations_analyzed, 5);
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_resolution_suggestions() {
        let use_case = create_test_use_case(true, true, true, true);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let options = ConflictAnalysisOptions {
            include_explanations: Some(true),
            ..Default::default()
        };
        
        let request = DetectPolicyConflictsRequest::new(policies).with_options(options);
        let result = use_case.execute(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap();
        
        // Check that resolution suggestions are provided
        for conflict in &response.conflict_analysis.conflicts {
            assert!(conflict.suggested_resolution.is_some());
            assert!(conflict.suggested_resolution.as_ref().unwrap().contains("Mock resolution"));
        }
        
        for redundancy in &response.conflict_analysis.redundancies {
            assert!(!redundancy.explanation.is_empty());
            assert!(redundancy.explanation.contains("Mock redundancy explanation"));
        }
        
        for unreachable in &response.conflict_analysis.unreachable_policies {
            assert!(!unreachable.explanation.is_empty());
            assert!(unreachable.explanation.contains("Mock unreachability explanation"));
        }
    }

    #[tokio::test]
    async fn test_detect_conflicts_use_case_combinations_calculation() {
        let use_case = create_test_use_case(false, false, false, false);
        
        // Test combinations calculation for different policy counts
        assert_eq!(use_case.calculate_combinations_analyzed(2), 5);  // 1 + 4
        assert_eq!(use_case.calculate_combinations_analyzed(3), 12); // 3 + 9
        assert_eq!(use_case.calculate_combinations_analyzed(4), 22); // 6 + 16
    }

    #[test]
    fn test_conflict_summary_generation() {
        let use_case = create_test_use_case(false, false, false, false);
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let conflicts = vec![
            PolicyConflict {
                conflict_type: ConflictType::DirectContradiction,
                involved_policies: vec![],
                description: "Test conflict".to_string(),
                severity: ConflictSeverity::High,
                suggested_resolution: None,
                location: None,
            }
        ];
        
        let redundancies = vec![
            PolicyRedundancy {
                redundant_policy: PolicyReference::new("policy2".to_string()),
                superseding_policies: vec![],
                explanation: "Test redundancy".to_string(),
                confidence: 0.9,
            }
        ];
        
        let unreachable = vec![];
        
        let summary = use_case.generate_conflict_summary(&policies, &conflicts, &redundancies, &unreachable);
        
        assert_eq!(summary.total_policies, 2);
        assert_eq!(summary.total_conflicts, 1);
        assert_eq!(summary.total_redundancies, 1);
        assert_eq!(summary.total_unreachable, 0);
        assert_eq!(summary.conflict_score, 1.0); // (1 + 1 + 0) / 2 = 1.0
    }
}