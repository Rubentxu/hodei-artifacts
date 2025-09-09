// crates/iam/src/features/detect_policy_conflicts/ports.rs

use crate::infrastructure::errors::IamError;
use super::dto::*;
use async_trait::async_trait;

/// Port for comprehensive policy conflict detection
#[async_trait]
pub trait PolicyConflictDetectionService: Send + Sync {
    /// Detect conflicts between multiple policies
    async fn detect_conflicts(&self, request: DetectPolicyConflictsRequest) -> Result<DetectPolicyConflictsResponse, IamError>;
}

/// Port for detecting direct policy conflicts
#[async_trait]
pub trait DirectConflictDetector: Send + Sync {
    /// Detect direct contradictions between policies
    async fn detect_direct_conflicts(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError>;
    
    /// Check if two specific policies conflict
    async fn check_policy_pair_conflict(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<Option<PolicyConflict>, IamError>;
}

/// Port for detecting policy redundancies
#[async_trait]
pub trait PolicyRedundancyDetector: Send + Sync {
    /// Detect redundant policies that can be removed
    async fn detect_redundancies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyRedundancy>, IamError>;
    
    /// Check if a policy is redundant given other policies
    async fn is_policy_redundant(&self, target_policy: &PolicyForAnalysis, other_policies: &[PolicyForAnalysis]) -> Result<Option<PolicyRedundancy>, IamError>;
    
    /// Calculate redundancy confidence score
    async fn calculate_redundancy_confidence(&self, redundant_policy: &PolicyForAnalysis, superseding_policies: &[PolicyForAnalysis]) -> Result<f64, IamError>;
}

/// Port for detecting unreachable policies
#[async_trait]
pub trait UnreachablePolicyDetector: Send + Sync {
    /// Detect policies that are unreachable due to conflicts
    async fn detect_unreachable_policies(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<UnreachablePolicy>, IamError>;
    
    /// Check if a policy is unreachable
    async fn is_policy_unreachable(&self, target_policy: &PolicyForAnalysis, other_policies: &[PolicyForAnalysis]) -> Result<Option<UnreachablePolicy>, IamError>;
    
    /// Find reachability conditions for a policy
    async fn find_reachability_conditions(&self, policy: &PolicyForAnalysis, blocking_policies: &[PolicyForAnalysis]) -> Result<Option<String>, IamError>;
}

/// Port for policy overlap analysis
#[async_trait]
pub trait PolicyOverlapAnalyzer: Send + Sync {
    /// Analyze overlapping permissions between policies
    async fn analyze_permission_overlaps(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<PolicyConflict>, IamError>;
    
    /// Calculate overlap score between two policies
    async fn calculate_overlap_score(&self, policy1: &PolicyForAnalysis, policy2: &PolicyForAnalysis) -> Result<f64, IamError>;
    
    /// Find common patterns between policies
    async fn find_common_patterns(&self, policies: &[PolicyForAnalysis]) -> Result<Vec<String>, IamError>;
}

/// Port for conflict metrics collection
#[async_trait]
pub trait ConflictAnalysisMetricsCollector: Send + Sync {
    /// Start metrics collection for conflict analysis
    async fn start_analysis_metrics(&self, operation_id: &str) -> Result<(), IamError>;
    
    /// Record analysis step metrics
    async fn record_analysis_step(&self, operation_id: &str, step_name: &str, duration_ms: u64) -> Result<(), IamError>;
    
    /// Record combinations analyzed
    async fn record_combinations_analyzed(&self, operation_id: &str, count: u64) -> Result<(), IamError>;
    
    /// Finish metrics collection and return results
    async fn finish_analysis_metrics(&self, operation_id: &str) -> Result<ConflictAnalysisMetrics, IamError>;
}

/// Port for conflict resolution suggestions
pub trait ConflictResolutionSuggester: Send + Sync {
    /// Generate resolution suggestions for conflicts
    fn suggest_conflict_resolution(&self, conflict: &PolicyConflict) -> Option<String>;
    
    /// Generate explanation for redundancy
    fn explain_redundancy(&self, redundancy: &PolicyRedundancy) -> String;
    
    /// Generate explanation for unreachable policy
    fn explain_unreachability(&self, unreachable: &UnreachablePolicy) -> String;
    
    /// Suggest policy priority adjustments
    fn suggest_priority_adjustments(&self, conflicts: &[PolicyConflict]) -> Vec<PriorityAdjustment>;
}

/// Port for conflict analysis configuration
pub trait ConflictAnalysisConfigProvider: Send + Sync {
    /// Get default analysis options
    fn get_default_options(&self) -> ConflictAnalysisOptions;
    
    /// Get analysis timeout
    fn get_analysis_timeout(&self) -> u64;
    
    /// Check if analysis type is enabled
    fn is_analysis_enabled(&self, analysis_type: AnalysisType) -> bool;
    
    /// Get performance thresholds
    fn get_performance_thresholds(&self) -> AnalysisPerformanceThresholds;
    
    /// Get redundancy detection threshold
    fn get_redundancy_threshold(&self) -> f64;
}

/// Port for conflict analysis result persistence
#[async_trait]
pub trait ConflictAnalysisRepository: Send + Sync {
    /// Store conflict analysis result
    async fn store_analysis_result(&self, policies: &[PolicyForAnalysis], result: &DetectPolicyConflictsResponse) -> Result<String, IamError>;
    
    /// Retrieve analysis result by ID
    async fn get_analysis_result(&self, result_id: &str) -> Result<Option<DetectPolicyConflictsResponse>, IamError>;
    
    /// Get analysis history for policies
    async fn get_analysis_history(&self, policy_ids: &[String]) -> Result<Vec<AnalysisHistoryEntry>, IamError>;
    
    /// Clean up old analysis results
    async fn cleanup_old_results(&self, older_than_days: u32) -> Result<u32, IamError>;
}

/// Types of conflict analysis
#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisType {
    DirectConflicts,
    Redundancies,
    UnreachablePolicies,
    PermissionOverlaps,
    CircularDependencies,
}

/// Performance thresholds for analysis
#[derive(Debug, Clone)]
pub struct AnalysisPerformanceThresholds {
    pub max_analysis_time_ms: u64,
    pub max_memory_usage_bytes: u64,
    pub max_combinations: u64,
    pub warning_time_ms: u64,
}

/// Priority adjustment suggestion
#[derive(Debug, Clone)]
pub struct PriorityAdjustment {
    pub policy_id: String,
    pub current_priority: Option<u32>,
    pub suggested_priority: u32,
    pub reason: String,
}

/// Analysis history entry
#[derive(Debug, Clone)]
pub struct AnalysisHistoryEntry {
    pub analysis_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub policy_ids: Vec<String>,
    pub conflicts_found: usize,
    pub analysis_duration_ms: u64,
}

/// Context for conflict analysis
#[derive(Debug, Clone)]
pub struct ConflictAnalysisContext {
    pub request_id: String,
    pub user_id: Option<String>,
    pub organization_id: Option<String>,
    pub analysis_timestamp: chrono::DateTime<chrono::Utc>,
    pub additional_context: std::collections::HashMap<String, String>,
}

impl Default for AnalysisPerformanceThresholds {
    fn default() -> Self {
        Self {
            max_analysis_time_ms: 30000,  // 30 seconds
            max_memory_usage_bytes: 500 * 1024 * 1024,  // 500MB
            max_combinations: 1000000,  // 1 million combinations
            warning_time_ms: 5000,  // 5 seconds
        }
    }
}

impl ConflictAnalysisContext {
    pub fn new(request_id: String) -> Self {
        Self {
            request_id,
            user_id: None,
            organization_id: None,
            analysis_timestamp: chrono::Utc::now(),
            additional_context: std::collections::HashMap::new(),
        }
    }

    pub fn with_user(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_organization(mut self, organization_id: String) -> Self {
        self.organization_id = Some(organization_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_performance_thresholds_default() {
        let thresholds = AnalysisPerformanceThresholds::default();
        assert_eq!(thresholds.max_analysis_time_ms, 30000);
        assert_eq!(thresholds.warning_time_ms, 5000);
        assert_eq!(thresholds.max_combinations, 1000000);
    }

    #[test]
    fn test_conflict_analysis_context_creation() {
        let context = ConflictAnalysisContext::new("test-123".to_string())
            .with_user("user-456".to_string())
            .with_organization("org-789".to_string());
        
        assert_eq!(context.request_id, "test-123");
        assert_eq!(context.user_id, Some("user-456".to_string()));
        assert_eq!(context.organization_id, Some("org-789".to_string()));
    }

    #[test]
    fn test_analysis_type_variants() {
        let analysis_types = vec![
            AnalysisType::DirectConflicts,
            AnalysisType::Redundancies,
            AnalysisType::UnreachablePolicies,
            AnalysisType::PermissionOverlaps,
            AnalysisType::CircularDependencies,
        ];
        
        assert_eq!(analysis_types.len(), 5);
    }

    #[test]
    fn test_priority_adjustment() {
        let adjustment = PriorityAdjustment {
            policy_id: "policy-123".to_string(),
            current_priority: Some(5),
            suggested_priority: 10,
            reason: "Resolve conflict with policy-456".to_string(),
        };
        
        assert_eq!(adjustment.policy_id, "policy-123");
        assert_eq!(adjustment.current_priority, Some(5));
        assert_eq!(adjustment.suggested_priority, 10);
    }
}