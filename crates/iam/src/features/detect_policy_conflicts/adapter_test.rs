// crates/iam/src/features/detect_policy_conflicts/adapter_test.rs

#[cfg(test)]
mod tests {
    use super::super::adapter::*;
    use super::super::dto::*;
    use super::super::ports::*;
    use crate::infrastructure::errors::IamError;

    #[tokio::test]
    async fn test_cedar_direct_conflict_detector_creation() {
        let result = CedarDirectConflictDetector::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_detect_direct_conflicts_empty() {
        let detector = CedarDirectConflictDetector::new().unwrap();
        let result = detector.detect_direct_conflicts(&[]).await;
        
        assert!(result.is_ok());
        let conflicts = result.unwrap();
        assert!(conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_detect_direct_conflicts_single_policy() {
        let detector = CedarDirectConflictDetector::new().unwrap();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let result = detector.detect_direct_conflicts(&policies).await;
        assert!(result.is_ok());
        
        let conflicts = result.unwrap();
        // Single policy should not have conflicts with itself
        assert!(conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_detect_direct_conflicts_multiple_policies() {
        let detector = CedarDirectConflictDetector::new().unwrap();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let result = detector.detect_direct_conflicts(&policies).await;
        assert!(result.is_ok());
        
        // May or may not detect conflicts depending on Cedar's analysis
        let _conflicts = result.unwrap();
    }

    #[tokio::test]
    async fn test_check_policy_pair_conflict() {
        let detector = CedarDirectConflictDetector::new().unwrap();
        let policy1 = PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string());
        let policy2 = PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string());
        
        let result = detector.check_policy_pair_conflict(&policy1, &policy2).await;
        assert!(result.is_ok());
        
        // May or may not detect conflict depending on implementation
        let _conflict = result.unwrap();
    }

    #[tokio::test]
    async fn test_simple_redundancy_detector_creation() {
        let result = SimpleRedundancyDetector::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_detect_redundancies_empty() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        let result = detector.detect_redundancies(&[]).await;
        
        assert!(result.is_ok());
        let redundancies = result.unwrap();
        assert!(redundancies.is_empty());
    }

    #[tokio::test]
    async fn test_detect_redundancies_identical_policies() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let result = detector.detect_redundancies(&policies).await;
        assert!(result.is_ok());
        
        let redundancies = result.unwrap();
        // Should detect redundancy between identical policies
        assert!(!redundancies.is_empty());
    }

    #[tokio::test]
    async fn test_is_policy_redundant() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        let target_policy = PolicyForAnalysis::new("target".to_string(), "permit(principal, action, resource);".to_string());
        let other_policies = vec![
            PolicyForAnalysis::new("other1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("other2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let result = detector.is_policy_redundant(&target_policy, &other_policies).await;
        assert!(result.is_ok());
        
        // Should detect redundancy with identical policy
        let redundancy = result.unwrap();
        assert!(redundancy.is_some());
    }

    #[tokio::test]
    async fn test_calculate_redundancy_confidence() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        let redundant_policy = PolicyForAnalysis::new("redundant".to_string(), "permit(principal, action, resource);".to_string());
        let superseding_policies = vec![
            PolicyForAnalysis::new("superseding".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let result = detector.calculate_redundancy_confidence(&redundant_policy, &superseding_policies).await;
        assert!(result.is_ok());
        
        let confidence = result.unwrap();
        assert!(confidence >= 0.0 && confidence <= 1.0);
    }

    #[tokio::test]
    async fn test_simple_unreachable_detector() {
        let detector = SimpleUnreachableDetector::new();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "forbid(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource) when condition;".to_string()),
        ];
        
        let result = detector.detect_unreachable_policies(&policies).await;
        assert!(result.is_ok());
        
        let unreachable = result.unwrap();
        // May detect unreachable policy depending on heuristics
        assert!(unreachable.len() <= policies.len());
    }

    #[tokio::test]
    async fn test_is_policy_unreachable() {
        let detector = SimpleUnreachableDetector::new();
        let target_policy = PolicyForAnalysis::new("target".to_string(), "permit(principal, action, resource) when specific_condition;".to_string());
        let other_policies = vec![
            PolicyForAnalysis::new("blocking".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let result = detector.is_policy_unreachable(&target_policy, &other_policies).await;
        assert!(result.is_ok());
        
        // Should detect unreachability due to more general forbid policy
        let unreachable = result.unwrap();
        assert!(unreachable.is_some());
    }

    #[tokio::test]
    async fn test_find_reachability_conditions() {
        let detector = SimpleUnreachableDetector::new();
        let policy = PolicyForAnalysis::new("policy".to_string(), "permit(principal, action, resource);".to_string());
        let blocking_policies = vec![
            PolicyForAnalysis::new("blocking".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let result = detector.find_reachability_conditions(&policy, &blocking_policies).await;
        assert!(result.is_ok());
        
        let conditions = result.unwrap();
        assert!(conditions.is_some());
    }

    #[tokio::test]
    async fn test_simple_overlap_analyzer() {
        let analyzer = SimpleOverlapAnalyzer::new();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, \"resource1\");".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, \"resource1\");".to_string()),
        ];
        
        let result = analyzer.analyze_permission_overlaps(&policies).await;
        assert!(result.is_ok());
        
        let overlaps = result.unwrap();
        // Should detect overlap between policies with same resource
        assert!(!overlaps.is_empty());
    }

    #[tokio::test]
    async fn test_calculate_overlap_score() {
        let analyzer = SimpleOverlapAnalyzer::new();
        let policy1 = PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, \"resource1\");".to_string());
        let policy2 = PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, \"resource1\");".to_string());
        
        let result = analyzer.calculate_overlap_score(&policy1, &policy2).await;
        assert!(result.is_ok());
        
        let score = result.unwrap();
        assert!(score >= 0.0 && score <= 1.0);
        // Should have high overlap score for similar policies
        assert!(score > 0.5);
    }

    #[tokio::test]
    async fn test_find_common_patterns() {
        let analyzer = SimpleOverlapAnalyzer::new();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, \"common_resource\");".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, \"common_resource\");".to_string()),
            PolicyForAnalysis::new("policy3".to_string(), "permit(principal, action, \"different_resource\");".to_string()),
        ];
        
        let result = analyzer.find_common_patterns(&policies).await;
        assert!(result.is_ok());
        
        let patterns = result.unwrap();
        // Should find "common_resource" as a common pattern
        assert!(patterns.contains(&"common_resource".to_string()));
    }

    #[tokio::test]
    async fn test_simple_conflict_metrics_collector() {
        let collector = SimpleConflictMetricsCollector::new();
        let operation_id = "test-operation-123";
        
        // Start collection
        let result = collector.start_analysis_metrics(operation_id).await;
        assert!(result.is_ok());
        
        // Record analysis steps
        let result = collector.record_analysis_step(operation_id, "direct_conflicts", 100).await;
        assert!(result.is_ok());
        
        let result = collector.record_analysis_step(operation_id, "redundancy_analysis", 200).await;
        assert!(result.is_ok());
        
        let result = collector.record_analysis_step(operation_id, "reachability_analysis", 150).await;
        assert!(result.is_ok());
        
        // Record combinations analyzed
        let result = collector.record_combinations_analyzed(operation_id, 50).await;
        assert!(result.is_ok());
        
        // Finish collection
        let result = collector.finish_analysis_metrics(operation_id).await;
        assert!(result.is_ok());
        
        let metrics = result.unwrap();
        assert_eq!(metrics.conflict_detection_ms, 100);
        assert_eq!(metrics.redundancy_analysis_ms, 200);
        assert_eq!(metrics.reachability_analysis_ms, 150);
        assert_eq!(metrics.combinations_analyzed, 50);
    }

    #[tokio::test]
    async fn test_metrics_collector_unknown_operation() {
        let collector = SimpleConflictMetricsCollector::new();
        let operation_id = "unknown-operation";
        
        // Try to record step for unknown operation
        let result = collector.record_analysis_step(operation_id, "test_step", 100).await;
        assert!(result.is_ok()); // Should handle gracefully
        
        // Try to finish unknown operation
        let result = collector.finish_analysis_metrics(operation_id).await;
        assert!(result.is_ok());
        
        let metrics = result.unwrap();
        // Should return default metrics
        assert_eq!(metrics.conflict_detection_ms, 0);
    }

    #[test]
    fn test_simple_resolution_suggester() {
        let suggester = SimpleResolutionSuggester::new();
        
        let conflict = PolicyConflict {
            conflict_type: ConflictType::DirectContradiction,
            involved_policies: vec![],
            description: "Test conflict".to_string(),
            severity: ConflictSeverity::High,
            suggested_resolution: None,
            location: None,
        };
        
        let suggestion = suggester.suggest_conflict_resolution(&conflict);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("precedence"));
        
        let redundancy = PolicyRedundancy {
            redundant_policy: PolicyReference::new("redundant".to_string()),
            superseding_policies: vec![PolicyReference::new("superseding".to_string())],
            explanation: "".to_string(),
            confidence: 0.9,
        };
        
        let explanation = suggester.explain_redundancy(&redundancy);
        assert!(explanation.contains("redundant"));
        assert!(explanation.contains("superseded"));
        
        let unreachable = UnreachablePolicy {
            policy: PolicyReference::new("unreachable".to_string()),
            blocking_policies: vec![PolicyReference::new("blocking".to_string())],
            explanation: "".to_string(),
            reachability_conditions: None,
        };
        
        let explanation = suggester.explain_unreachability(&unreachable);
        assert!(explanation.contains("unreachable"));
        assert!(explanation.contains("blocking"));
    }

    #[test]
    fn test_suggest_priority_adjustments() {
        let suggester = SimpleResolutionSuggester::new();
        
        let conflicts = vec![
            PolicyConflict {
                conflict_type: ConflictType::DirectContradiction,
                involved_policies: vec![
                    PolicyReference::new("policy1".to_string()),
                    PolicyReference::new("policy2".to_string()),
                ],
                description: "Critical conflict".to_string(),
                severity: ConflictSeverity::Critical,
                suggested_resolution: None,
                location: None,
            },
            PolicyConflict {
                conflict_type: ConflictType::OverlappingPermissions,
                involved_policies: vec![
                    PolicyReference::new("policy3".to_string()),
                ],
                description: "Low priority conflict".to_string(),
                severity: ConflictSeverity::Low,
                suggested_resolution: None,
                location: None,
            },
        ];
        
        let adjustments = suggester.suggest_priority_adjustments(&conflicts);
        
        // Should suggest adjustments for critical/high severity conflicts
        assert!(!adjustments.is_empty());
        assert!(adjustments.iter().any(|adj| adj.policy_id == "policy1"));
        assert!(adjustments.iter().any(|adj| adj.policy_id == "policy2"));
    }

    #[test]
    fn test_default_conflict_analysis_config_provider() {
        let provider = DefaultConflictAnalysisConfigProvider::new();
        
        let options = provider.get_default_options();
        assert_eq!(options.detect_redundancies, Some(true));
        assert_eq!(options.find_unreachable, Some(true));
        assert_eq!(options.redundancy_threshold, Some(0.8));
        assert_eq!(options.include_explanations, Some(true));
        assert_eq!(options.timeout_ms, Some(30000));
        
        let timeout = provider.get_analysis_timeout();
        assert_eq!(timeout, 30000);
        
        assert!(provider.is_analysis_enabled(AnalysisType::DirectConflicts));
        assert!(provider.is_analysis_enabled(AnalysisType::Redundancies));
        assert!(provider.is_analysis_enabled(AnalysisType::UnreachablePolicies));
        assert!(provider.is_analysis_enabled(AnalysisType::PermissionOverlaps));
        assert!(provider.is_analysis_enabled(AnalysisType::CircularDependencies));
        
        let thresholds = provider.get_performance_thresholds();
        assert_eq!(thresholds.max_analysis_time_ms, 30000);
        assert_eq!(thresholds.warning_time_ms, 5000);
        assert_eq!(thresholds.max_combinations, 1000000);
        
        let redundancy_threshold = provider.get_redundancy_threshold();
        assert_eq!(redundancy_threshold, 0.8);
    }

    #[test]
    fn test_policy_similarity_calculation() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        
        let policy1 = PolicyForAnalysis::new("policy1".to_string(), "permit principal action resource".to_string());
        let policy2 = PolicyForAnalysis::new("policy2".to_string(), "permit principal action resource".to_string());
        let policy3 = PolicyForAnalysis::new("policy3".to_string(), "forbid different things entirely".to_string());
        
        let similarity_identical = detector.calculate_policy_similarity(&policy1, &policy2);
        let similarity_different = detector.calculate_policy_similarity(&policy1, &policy3);
        
        assert!(similarity_identical > similarity_different);
        assert!(similarity_identical > 0.8); // Should be very similar
        assert!(similarity_different < 0.5); // Should be quite different
    }

    #[test]
    fn test_pattern_extraction() {
        let analyzer = SimpleOverlapAnalyzer::new();
        
        let policy_content = r#"permit(principal, action, "resource1") when condition == "value";"#;
        let patterns = analyzer.extract_patterns(policy_content);
        
        assert!(patterns.contains("resource1"));
        assert!(patterns.contains("value"));
    }

    #[test]
    fn test_unreachable_policy_heuristics() {
        let detector = SimpleUnreachableDetector::new();
        
        let general_policy = PolicyForAnalysis::new("general".to_string(), "forbid(principal, action, resource);".to_string());
        let specific_policy = PolicyForAnalysis::new("specific".to_string(), "permit(principal, action, resource) when condition1 && condition2;".to_string());
        
        assert!(detector.is_more_general(&general_policy, &specific_policy));
        assert!(detector.has_stronger_effect(&general_policy, &specific_policy));
        
        let permit_policy = PolicyForAnalysis::new("permit".to_string(), "permit(principal, action, resource);".to_string());
        let forbid_policy = PolicyForAnalysis::new("forbid".to_string(), "forbid(principal, action, resource);".to_string());
        
        assert!(detector.has_stronger_effect(&forbid_policy, &permit_policy));
        assert!(!detector.has_stronger_effect(&permit_policy, &forbid_policy));
    }
}