// crates/iam/src/features/detect_policy_conflicts/adapter.rs

use crate::infrastructure::errors::IamError;
use super::dto::*;
use super::ports::*;
use security::ComprehensiveCedarValidator;
use async_trait::async_trait;
use std::sync::Arc;
use std::collections::HashMap;


/// Cedar-based implementation of direct conflict detection
pub struct CedarDirectConflictDetector {
    validator: Arc<ComprehensiveCedarValidator>,
}

impl CedarDirectConflictDetector {
    pub fn new() -> Result<Self, IamError> {
        let validator = Arc::new(
            ComprehensiveCedarValidator::new()
                .map_err(|e| IamError::ConfigurationError(format!("Failed to create Cedar validator: {}", e)))?
        );

        Ok(Self { validator })
    }

    pub fn with_validator(validator: Arc<ComprehensiveCedarValidator>) -> Self {
        Self { validator }
    }

    /// Analyze two policies for direct conflicts
    async fn analyze_policy_pair(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
        // Create a combined policy set to test for conflicts
        let combined_policies = format!("{}\n{}", policy1.content, policy2.content);
        
        let validation_result = self.validator
            .validate_policy_comprehensive(&combined_policies)
            .await
            .map_err(|e| IamError::validation_error(format!("Cedar validation failed: {}", e)))?;

        // Check for semantic errors that indicate conflicts
        for error in &validation_result.semantic_errors {
            if self.is_conflict_error(error) {
                return Ok(Some(PolicyConflict {
                    conflict_type: self.classify_conflict_type(error),
                    involved_policies: vec![
                        PolicyReference::new(policy1.id.clone()).with_name(policy1.name.clone().unwrap_or_default()),
                        PolicyReference::new(policy2.id.clone()).with_name(policy2.name.clone().unwrap_or_default()),
                    ],
                    description: error.clone(),
                    severity: self.determine_conflict_severity(error),
                    suggested_resolution: None, // Will be filled by resolution suggester
                    location: None, // Cedar doesn't provide detailed location info in current setup
                }));
            }
        }

        // Check for logical conflicts by analyzing policy effects
        if let Some(logical_conflict) = self.detect_logical_conflict(policy1, policy2).await? {
            return Ok(Some(logical_conflict));
        }

        Ok(None)
    }

    fn is_conflict_error(&self, error: &str) -> bool {
        error.contains("conflict") || 
        error.contains("contradiction") || 
        error.contains("ambiguous") ||
        error.contains("overlapping")
    }

    fn classify_conflict_type(&self, error: &str) -> ConflictType {
        if error.contains("contradiction") {
            ConflictType::DirectContradiction
        } else if error.contains("overlapping") {
            ConflictType::OverlappingPermissions
        } else if error.contains("ambiguous") {
            ConflictType::AmbiguousPrecedence
        } else if error.contains("circular") {
            ConflictType::CircularDependency
        } else if error.contains("condition") {
            ConflictType::InconsistentConditions
        } else if error.contains("resource") {
            ConflictType::ResourceAccessConflict
        } else if error.contains("action") {
            ConflictType::ActionPermissionConflict
        } else {
            ConflictType::DirectContradiction
        }
    }

    fn determine_conflict_severity(&self, error: &str) -> ConflictSeverity {
        if error.contains("critical") || error.contains("contradiction") {
            ConflictSeverity::Critical
        } else if error.contains("high") || error.contains("security") {
            ConflictSeverity::High
        } else if error.contains("medium") || error.contains("performance") {
            ConflictSeverity::Medium
        } else if error.contains("warning") {
            ConflictSeverity::Low
        } else {
            ConflictSeverity::Info
        }
    }

    async fn detect_logical_conflict(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
        // Simple heuristic: check if one policy permits and another forbids similar patterns
        let policy1_permits = policy1.content.contains("permit");
        let policy1_forbids = policy1.content.contains("forbid");
        let policy2_permits = policy2.content.contains("permit");
        let policy2_forbids = policy2.content.contains("forbid");

        // Check for direct permit/forbid conflicts on similar patterns
        if (policy1_permits && policy2_forbids) || (policy1_forbids && policy2_permits) {
            // This is a simplified check - in a real implementation, we'd parse the policies
            // and check if they apply to the same principal/action/resource patterns
            if self.have_overlapping_patterns(policy1, policy2) {
                return Ok(Some(PolicyConflict {
                    conflict_type: ConflictType::DirectContradiction,
                    involved_policies: vec![
                        PolicyReference::new(policy1.id.clone()),
                        PolicyReference::new(policy2.id.clone()),
                    ],
                    description: "Policies have conflicting effects (permit vs forbid) on overlapping patterns".to_string(),
                    severity: ConflictSeverity::High,
                    suggested_resolution: Some("Review policy precedence or refine policy conditions".to_string()),
                    location: None,
                }));
            }
        }

        Ok(None)
    }

    fn have_overlapping_patterns(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> bool {
        // Simplified pattern matching - in a real implementation, this would parse Cedar policies
        // and compare principal, action, and resource patterns
        
        // Check for common keywords that might indicate overlap
        let common_keywords = ["principal", "action", "resource", "when", "unless"];
        
        for keyword in &common_keywords {
            if policy1.content.contains(keyword) && policy2.content.contains(keyword) {
                return true;
            }
        }
        
        false
    }
}

#[async_trait]
impl DirectConflictDetector for CedarDirectConflictDetector {
    async fn detect_direct_conflicts(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
        let mut conflicts = Vec::new();

        // Compare each pair of policies
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                if let Some(conflict) = self.check_policy_pair_conflict(&policies[i], &policies[j]).await? {
                    conflicts.push(conflict);
                }
            }
        }

        Ok(conflicts)
    }

    async fn check_policy_pair_conflict(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError> {
        self.analyze_policy_pair(policy1, policy2).await
    }
}

/// Simple implementation of redundancy detection
pub struct SimpleRedundancyDetector {
    validator: Arc<ComprehensiveCedarValidator>,
}

impl SimpleRedundancyDetector {
    pub fn new() -> Result<Self, IamError> {
        let validator = Arc::new(
            ComprehensiveCedarValidator::new()
                .map_err(|e| IamError::ConfigurationError(format!("Failed to create Cedar validator: {}", e)))?
        );

        Ok(Self { validator })
    }

    /// Check if removing a policy changes the overall policy set behavior
    async fn would_removal_change_behavior(&self, target_policy: &PolicyForAnalysis, other_policies: &[PolicyForAnalysis]) -> Result<bool, IamError> {
        // Create policy set with all policies
        let all_policies = {
            let mut all = other_policies.to_vec();
            all.push(target_policy.clone());
            all
        };

        // Create policy set without target policy
        let without_target = other_policies;

        // In a real implementation, we would test various authorization scenarios
        // to see if removing the target policy changes any decisions
        // For now, we use a simplified heuristic
        
        let all_content: String = all_policies.iter().map(|p| &p.content).cloned().collect::<Vec<_>>().join("\n");
        let without_content: String = without_target.iter().map(|p| &p.content).cloned().collect::<Vec<_>>().join("\n");

        // If the policies are identical without the target, it might be redundant
        Ok(all_content.len() != without_content.len())
    }
}

#[async_trait]
impl PolicyRedundancyDetector for SimpleRedundancyDetector {
    async fn detect_redundancies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyRedundancy>, IamError> {
        let mut redundancies = Vec::new();

        for (i, policy) in policies.iter().enumerate() {
            let other_policies: Vec<PolicyForAnalysis> = policies.iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, p)| p.clone())
                .collect();

            if let Some(redundancy) = self.is_policy_redundant(policy, &other_policies).await? {
                redundancies.push(redundancy);
            }
        }

        Ok(redundancies)
    }

    async fn is_policy_redundant(&self, target_policy: &PolicyForAnalysis, other_policies: &[PolicyForAnalysis]) -> Result<Option<PolicyRedundancy>, IamError> {
        // Simple heuristic: if policy content is very similar to others, it might be redundant
        let mut similar_policies = Vec::new();
        
        for other_policy in other_policies {
            let similarity = self.calculate_policy_similarity(target_policy, other_policy);
            if similarity > 0.8 { // 80% similarity threshold
                similar_policies.push(PolicyReference::new(other_policy.id.clone()));
            }
        }

        if !similar_policies.is_empty() {
            let confidence = self.calculate_redundancy_confidence(target_policy, other_policies).await?;
            
            return Ok(Some(PolicyRedundancy {
                redundant_policy: PolicyReference::new(target_policy.id.clone()),
                superseding_policies: similar_policies,
                explanation: "Policy appears to be redundant based on content similarity".to_string(),
                confidence,
            }));
        }

        Ok(None)
    }

    async fn calculate_redundancy_confidence(&self, _redundant_policy: &PolicyForAnalysis, _superseding_policies: &[PolicyForAnalysis]) -> Result<f64, IamError> {
        // Simplified confidence calculation
        // In a real implementation, this would involve more sophisticated analysis
        Ok(0.85)
    }
}

impl SimpleRedundancyDetector {
    fn calculate_policy_similarity(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> f64 {
        // Simple string similarity based on common words
        let words1: std::collections::HashSet<&str> = policy1.content.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = policy2.content.split_whitespace().collect();
        
        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();
        
        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }
}

/// Simple implementation of unreachable policy detection
pub struct SimpleUnreachableDetector;

impl SimpleUnreachableDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl UnreachablePolicyDetector for SimpleUnreachableDetector {
    async fn detect_unreachable_policies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<UnreachablePolicy>, IamError> {
        let mut unreachable = Vec::new();

        // Simple heuristic: policies that come after more general policies might be unreachable
        for (i, policy) in policies.iter().enumerate() {
            let preceding_policies: Vec<PolicyForAnalysis> = policies.iter()
                .take(i)
                .cloned()
                .collect();

            if let Some(unreachable_policy) = self.is_policy_unreachable(policy, &preceding_policies).await? {
                unreachable.push(unreachable_policy);
            }
        }

        Ok(unreachable)
    }

    async fn is_policy_unreachable(&self, target_policy: &PolicyForAnalysis, other_policies: &[PolicyForAnalysis]) -> Result<Option<UnreachablePolicy>, IamError> {
        // Simple heuristic: if there are very general policies that would always match first
        for other_policy in other_policies {
            if self.is_more_general(other_policy, target_policy) && self.has_stronger_effect(other_policy, target_policy) {
                return Ok(Some(UnreachablePolicy {
                    policy: PolicyReference::new(target_policy.id.clone()),
                    blocking_policies: vec![PolicyReference::new(other_policy.id.clone())],
                    explanation: "Policy is unreachable due to more general preceding policy".to_string(),
                    reachability_conditions: Some("Add more specific conditions or reorder policies".to_string()),
                }));
            }
        }

        Ok(None)
    }

    async fn find_reachability_conditions(&self, _policy: &PolicyForAnalysis, _blocking_policies: &[PolicyForAnalysis]) -> Result<Option<String>, IamError> {
        Ok(Some("Consider adding more specific conditions or changing policy order".to_string()))
    }
}

impl SimpleUnreachableDetector {
    fn is_more_general(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> bool {
        // Simple heuristic: fewer conditions means more general
        let conditions1 = policy1.content.matches("when").count() + policy1.content.matches("unless").count();
        let conditions2 = policy2.content.matches("when").count() + policy2.content.matches("unless").count();
        
        conditions1 < conditions2
    }

    fn has_stronger_effect(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> bool {
        // forbid is stronger than permit
        let policy1_forbids = policy1.content.contains("forbid");
        let policy2_permits = policy2.content.contains("permit");
        
        policy1_forbids && policy2_permits
    }
}

/// Simple implementation of overlap analysis
pub struct SimpleOverlapAnalyzer;

impl SimpleOverlapAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyOverlapAnalyzer for SimpleOverlapAnalyzer {
    async fn analyze_permission_overlaps(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError> {
        let mut overlaps = Vec::new();

        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                let overlap_score = self.calculate_overlap_score(&policies[i], &policies[j]).await?;
                
                if overlap_score > 0.7 { // 70% overlap threshold
                    overlaps.push(PolicyConflict {
                        conflict_type: ConflictType::OverlappingPermissions,
                        involved_policies: vec![
                            PolicyReference::new(policies[i].id.clone()),
                            PolicyReference::new(policies[j].id.clone()),
                        ],
                        description: format!("Policies have {:.1}% overlapping permissions", overlap_score * 100.0),
                        severity: if overlap_score > 0.9 { ConflictSeverity::High } else { ConflictSeverity::Medium },
                        suggested_resolution: Some("Consider consolidating overlapping policies".to_string()),
                        location: None,
                    });
                }
            }
        }

        Ok(overlaps)
    }

    async fn calculate_overlap_score(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<f64, IamError> {
        // Simple overlap calculation based on common patterns
        let patterns1 = self.extract_patterns(&policy1.content);
        let patterns2 = self.extract_patterns(&policy2.content);
        
        let intersection = patterns1.intersection(&patterns2).count();
        let union = patterns1.union(&patterns2).count();
        
        Ok(if union == 0 { 0.0 } else { intersection as f64 / union as f64 })
    }

    async fn find_common_patterns(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<String>, IamError> {
        let mut pattern_counts: HashMap<String, usize> = HashMap::new();
        
        for policy in policies {
            let patterns = self.extract_patterns(&policy.content);
            for pattern in patterns {
                *pattern_counts.entry(pattern).or_insert(0) += 1;
            }
        }
        
        let common_patterns: Vec<String> = pattern_counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(pattern, _)| pattern)
            .collect();
        
        Ok(common_patterns)
    }
}

impl SimpleOverlapAnalyzer {
    fn extract_patterns(&self, policy_content: &str) -> std::collections::HashSet<String> {
        // Simple pattern extraction - in a real implementation, this would parse Cedar syntax
        let mut patterns = std::collections::HashSet::new();
        
        // Extract quoted strings as patterns
        let mut in_quotes = false;
        let mut current_pattern = String::new();
        
        for ch in policy_content.chars() {
            match ch {
                '"' => {
                    if in_quotes {
                        if !current_pattern.is_empty() {
                            patterns.insert(current_pattern.clone());
                            current_pattern.clear();
                        }
                    }
                    in_quotes = !in_quotes;
                }
                _ if in_quotes => {
                    current_pattern.push(ch);
                }
                _ => {}
            }
        }
        
        patterns
    }
}

/// Simple metrics collector for conflict analysis
pub struct SimpleConflictMetricsCollector {
    metrics: Arc<std::sync::RwLock<HashMap<String, ConflictAnalysisMetrics>>>,
}

impl SimpleConflictMetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ConflictAnalysisMetricsCollector for SimpleConflictMetricsCollector {
    async fn start_analysis_metrics(&self, operation_id: &str) -> Result<(), IamError> {
        let mut metrics = self.metrics.write()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to acquire metrics lock: {}", e)))?;
        
        metrics.insert(operation_id.to_string(), ConflictAnalysisMetrics::default());
        Ok(())
    }

    async fn record_analysis_step(&self, operation_id: &str, step_name: &str, duration_ms: u64) -> Result<(), IamError> {
        let mut metrics = self.metrics.write()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to acquire metrics lock: {}", e)))?;
        
        if let Some(metric) = metrics.get_mut(operation_id) {
            match step_name {
                "direct_conflicts" => metric.conflict_detection_ms = duration_ms,
                "redundancy_analysis" => metric.redundancy_analysis_ms = duration_ms,
                "reachability_analysis" => metric.reachability_analysis_ms = duration_ms,
                _ => {} // Ignore unknown steps
            }
        }
        
        Ok(())
    }

    async fn record_combinations_analyzed(&self, operation_id: &str, count: u64) -> Result<(), IamError> {
        let mut metrics = self.metrics.write()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to acquire metrics lock: {}", e)))?;
        
        if let Some(metric) = metrics.get_mut(operation_id) {
            metric.combinations_analyzed = count;
        }
        
        Ok(())
    }

    async fn finish_analysis_metrics(&self, operation_id: &str) -> Result<ConflictAnalysisMetrics, IamError> {
        let mut metrics = self.metrics.write()
            .map_err(|e| IamError::ConfigurationError(format!("Failed to acquire metrics lock: {}", e)))?;
        
        let result = metrics.remove(operation_id)
            .unwrap_or_else(ConflictAnalysisMetrics::default);
        
        Ok(result)
    }
}

/// Simple resolution suggester
pub struct SimpleResolutionSuggester;

impl SimpleResolutionSuggester {
    pub fn new() -> Self {
        Self
    }
}

impl ConflictResolutionSuggester for SimpleResolutionSuggester {
    fn suggest_conflict_resolution(&self, conflict: &PolicyConflict) -> Option<String> {
        match conflict.conflict_type {
            ConflictType::DirectContradiction => {
                Some("Review policy precedence rules or add more specific conditions to resolve the contradiction".to_string())
            }
            ConflictType::OverlappingPermissions => {
                Some("Consider consolidating overlapping policies or adding priority levels".to_string())
            }
            ConflictType::AmbiguousPrecedence => {
                Some("Define clear precedence rules or reorder policies".to_string())
            }
            ConflictType::CircularDependency => {
                Some("Break circular dependencies by restructuring policy conditions".to_string())
            }
            ConflictType::InconsistentConditions => {
                Some("Review and align policy conditions for consistency".to_string())
            }
            ConflictType::ResourceAccessConflict => {
                Some("Clarify resource access patterns and permissions".to_string())
            }
            ConflictType::ActionPermissionConflict => {
                Some("Review action permissions and ensure they are properly scoped".to_string())
            }
        }
    }

    fn explain_redundancy(&self, redundancy: &PolicyRedundancy) -> String {
        format!(
            "Policy '{}' appears to be redundant because it is superseded by {} other policies with similar effects",
            redundancy.redundant_policy.id,
            redundancy.superseding_policies.len()
        )
    }

    fn explain_unreachability(&self, unreachable: &UnreachablePolicy) -> String {
        format!(
            "Policy '{}' is unreachable because {} blocking policies will always match first",
            unreachable.policy.id,
            unreachable.blocking_policies.len()
        )
    }

    fn suggest_priority_adjustments(&self, conflicts: &[PolicyConflict]) -> Vec<PriorityAdjustment> {
        let mut adjustments = Vec::new();
        
        for (i, conflict) in conflicts.iter().enumerate() {
            if conflict.severity == ConflictSeverity::Critical || conflict.severity == ConflictSeverity::High {
                for policy_ref in &conflict.involved_policies {
                    adjustments.push(PriorityAdjustment {
                        policy_id: policy_ref.id.clone(),
                        current_priority: None,
                        suggested_priority: (i as u32 + 1) * 10,
                        reason: format!("Resolve {} conflict", format!("{:?}", conflict.conflict_type).to_lowercase()),
                    });
                }
            }
        }
        
        adjustments
    }
}

/// Simple configuration provider
pub struct DefaultConflictAnalysisConfigProvider;

impl DefaultConflictAnalysisConfigProvider {
    pub fn new() -> Self {
        Self
    }
}

impl ConflictAnalysisConfigProvider for DefaultConflictAnalysisConfigProvider {
    fn get_default_options(&self) -> ConflictAnalysisOptions {
        ConflictAnalysisOptions::default()
    }

    fn get_analysis_timeout(&self) -> u64 {
        30000 // 30 seconds
    }

    fn is_analysis_enabled(&self, analysis_type: AnalysisType) -> bool {
        match analysis_type {
            AnalysisType::DirectConflicts => true,
            AnalysisType::Redundancies => true,
            AnalysisType::UnreachablePolicies => true,
            AnalysisType::PermissionOverlaps => true,
            AnalysisType::CircularDependencies => true,
        }
    }

    fn get_performance_thresholds(&self) -> AnalysisPerformanceThresholds {
        AnalysisPerformanceThresholds::default()
    }

    fn get_redundancy_threshold(&self) -> f64 {
        0.8
    }
}

// Default implementations
impl Default for CedarDirectConflictDetector {
    fn default() -> Self {
        Self::new().expect("Failed to create default CedarDirectConflictDetector")
    }
}

impl Default for SimpleRedundancyDetector {
    fn default() -> Self {
        Self::new().expect("Failed to create default SimpleRedundancyDetector")
    }
}

impl Default for SimpleUnreachableDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleOverlapAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleConflictMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SimpleResolutionSuggester {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DefaultConflictAnalysisConfigProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cedar_direct_conflict_detector_creation() {
        let detector = CedarDirectConflictDetector::new();
        assert!(detector.is_ok());
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
    async fn test_simple_redundancy_detector() {
        let detector = SimpleRedundancyDetector::new().unwrap();
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()),
        ];
        
        let result = detector.detect_redundancies(&policies).await;
        assert!(result.is_ok());
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
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = SimpleConflictMetricsCollector::new();
        let operation_id = "test-123";
        
        collector.start_analysis_metrics(operation_id).await.unwrap();
        collector.record_analysis_step(operation_id, "direct_conflicts", 100).await.unwrap();
        collector.record_combinations_analyzed(operation_id, 50).await.unwrap();
        let metrics = collector.finish_analysis_metrics(operation_id).await.unwrap();
        
        assert_eq!(metrics.conflict_detection_ms, 100);
        assert_eq!(metrics.combinations_analyzed, 50);
    }

    #[test]
    fn test_resolution_suggester() {
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
    }

    #[test]
    fn test_config_provider() {
        let provider = DefaultConflictAnalysisConfigProvider::new();
        
        let options = provider.get_default_options();
        assert_eq!(options.detect_redundancies, Some(true));
        
        let timeout = provider.get_analysis_timeout();
        assert_eq!(timeout, 30000);
        
        assert!(provider.is_analysis_enabled(AnalysisType::DirectConflicts));
        
        let threshold = provider.get_redundancy_threshold();
        assert_eq!(threshold, 0.8);
    }
}