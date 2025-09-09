// crates/iam/src/features/detect_policy_conflicts/dto.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request to detect conflicts between policies
#[derive(Debug, Clone, Deserialize)]
pub struct DetectPolicyConflictsRequest {
    /// List of policies to analyze for conflicts
    pub policies: Vec<PolicyForAnalysis>,
    /// Optional analysis options
    pub options: Option<ConflictAnalysisOptions>,
    /// Optional context for analysis
    pub context: Option<HashMap<String, String>>,
}

/// Response from policy conflict detection
#[derive(Debug, Clone, Serialize)]
pub struct DetectPolicyConflictsResponse {
    /// Whether conflicts were detected
    pub has_conflicts: bool,
    /// Detailed conflict analysis results
    pub conflict_analysis: PolicyConflictAnalysis,
    /// Performance metrics
    pub metrics: ConflictAnalysisMetrics,
}

/// Individual policy for conflict analysis
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyForAnalysis {
    /// Unique identifier for the policy
    pub id: String,
    /// Policy content in Cedar format
    pub content: String,
    /// Optional policy name for better reporting
    pub name: Option<String>,
    /// Optional priority for conflict resolution
    pub priority: Option<u32>,
}

/// Detailed conflict analysis results
#[derive(Debug, Clone, Serialize)]
pub struct PolicyConflictAnalysis {
    /// List of detected conflicts
    pub conflicts: Vec<PolicyConflict>,
    /// Redundant policies that can be removed
    pub redundancies: Vec<PolicyRedundancy>,
    /// Policies that are unreachable due to conflicts
    pub unreachable_policies: Vec<UnreachablePolicy>,
    /// Summary statistics
    pub summary: ConflictSummary,
}

/// Individual policy conflict
#[derive(Debug, Clone, Serialize)]
pub struct PolicyConflict {
    /// Type of conflict detected
    pub conflict_type: ConflictType,
    /// Policies involved in the conflict
    pub involved_policies: Vec<PolicyReference>,
    /// Detailed description of the conflict
    pub description: String,
    /// Severity level of the conflict
    pub severity: ConflictSeverity,
    /// Suggested resolution
    pub suggested_resolution: Option<String>,
    /// Location information if available
    pub location: Option<ConflictLocation>,
}

/// Policy redundancy information
#[derive(Debug, Clone, Serialize)]
pub struct PolicyRedundancy {
    /// The redundant policy
    pub redundant_policy: PolicyReference,
    /// Policies that make this one redundant
    pub superseding_policies: Vec<PolicyReference>,
    /// Explanation of why it's redundant
    pub explanation: String,
    /// Confidence level of the redundancy detection
    pub confidence: f64,
}

/// Unreachable policy information
#[derive(Debug, Clone, Serialize)]
pub struct UnreachablePolicy {
    /// The unreachable policy
    pub policy: PolicyReference,
    /// Policies that block this one
    pub blocking_policies: Vec<PolicyReference>,
    /// Explanation of why it's unreachable
    pub explanation: String,
    /// Conditions under which it might be reachable
    pub reachability_conditions: Option<String>,
}

/// Reference to a policy in conflict analysis
#[derive(Debug, Clone, Serialize)]
pub struct PolicyReference {
    /// Policy ID
    pub id: String,
    /// Policy name if available
    pub name: Option<String>,
    /// Line number in the policy where conflict occurs
    pub line: Option<u32>,
    /// Column number where conflict occurs
    pub column: Option<u32>,
}

/// Summary of conflict analysis
#[derive(Debug, Clone, Serialize)]
pub struct ConflictSummary {
    /// Total number of policies analyzed
    pub total_policies: usize,
    /// Number of conflicts found
    pub total_conflicts: usize,
    /// Number of redundant policies
    pub total_redundancies: usize,
    /// Number of unreachable policies
    pub total_unreachable: usize,
    /// Overall conflict score (0.0 = no conflicts, 1.0 = maximum conflicts)
    pub conflict_score: f64,
}

/// Location information for conflicts
#[derive(Debug, Clone, Serialize)]
pub struct ConflictLocation {
    /// Principal pattern that conflicts
    pub principal_pattern: Option<String>,
    /// Action pattern that conflicts
    pub action_pattern: Option<String>,
    /// Resource pattern that conflicts
    pub resource_pattern: Option<String>,
    /// Condition that causes conflict
    pub condition: Option<String>,
}

/// Options for conflict analysis
#[derive(Debug, Clone, Deserialize)]
pub struct ConflictAnalysisOptions {
    /// Whether to detect redundancies
    pub detect_redundancies: Option<bool>,
    /// Whether to find unreachable policies
    pub find_unreachable: Option<bool>,
    /// Minimum confidence threshold for redundancy detection
    pub redundancy_threshold: Option<f64>,
    /// Whether to include detailed explanations
    pub include_explanations: Option<bool>,
    /// Maximum analysis time in milliseconds
    pub timeout_ms: Option<u64>,
}

/// Performance metrics for conflict analysis
#[derive(Debug, Clone, Serialize)]
pub struct ConflictAnalysisMetrics {
    /// Total analysis time in milliseconds
    pub total_duration_ms: u64,
    /// Time spent on conflict detection
    pub conflict_detection_ms: u64,
    /// Time spent on redundancy analysis
    pub redundancy_analysis_ms: u64,
    /// Time spent on reachability analysis
    pub reachability_analysis_ms: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: Option<u64>,
    /// Number of policy combinations analyzed
    pub combinations_analyzed: u64,
}

/// Types of policy conflicts
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum ConflictType {
    /// Direct contradiction (permit vs deny for same request)
    DirectContradiction,
    /// Overlapping permissions with different effects
    OverlappingPermissions,
    /// Ambiguous precedence
    AmbiguousPrecedence,
    /// Circular dependencies
    CircularDependency,
    /// Inconsistent conditions
    InconsistentConditions,
    /// Resource access conflicts
    ResourceAccessConflict,
    /// Action permission conflicts
    ActionPermissionConflict,
}

/// Severity levels for conflicts
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum ConflictSeverity {
    /// Critical conflict that must be resolved
    Critical,
    /// High priority conflict
    High,
    /// Medium priority conflict
    Medium,
    /// Low priority conflict or warning
    Low,
    /// Informational only
    Info,
}

// Implementations

impl DetectPolicyConflictsRequest {
    pub fn new(policies: Vec<PolicyForAnalysis>) -> Self {
        Self {
            policies,
            options: None,
            context: None,
        }
    }

    pub fn with_options(mut self, options: ConflictAnalysisOptions) -> Self {
        self.options = Some(options);
        self
    }

    pub fn with_context(mut self, context: HashMap<String, String>) -> Self {
        self.context = Some(context);
        self
    }
}

impl PolicyForAnalysis {
    pub fn new(id: String, content: String) -> Self {
        Self {
            id,
            content,
            name: None,
            priority: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = Some(priority);
        self
    }
}

impl DetectPolicyConflictsResponse {
    pub fn no_conflicts(metrics: ConflictAnalysisMetrics) -> Self {
        Self {
            has_conflicts: false,
            conflict_analysis: PolicyConflictAnalysis {
                conflicts: vec![],
                redundancies: vec![],
                unreachable_policies: vec![],
                summary: ConflictSummary {
                    total_policies: 0,
                    total_conflicts: 0,
                    total_redundancies: 0,
                    total_unreachable: 0,
                    conflict_score: 0.0,
                },
            },
            metrics,
        }
    }

    pub fn with_conflicts(conflict_analysis: PolicyConflictAnalysis, metrics: ConflictAnalysisMetrics) -> Self {
        let has_conflicts = !conflict_analysis.conflicts.is_empty() 
            || !conflict_analysis.redundancies.is_empty() 
            || !conflict_analysis.unreachable_policies.is_empty();

        Self {
            has_conflicts,
            conflict_analysis,
            metrics,
        }
    }

    pub fn get_conflict_summary(&self) -> String {
        let summary = &self.conflict_analysis.summary;
        if !self.has_conflicts {
            "No conflicts detected".to_string()
        } else {
            format!(
                "Found {} conflicts, {} redundancies, {} unreachable policies (score: {:.2})",
                summary.total_conflicts,
                summary.total_redundancies,
                summary.total_unreachable,
                summary.conflict_score
            )
        }
    }
}

impl Default for ConflictAnalysisOptions {
    fn default() -> Self {
        Self {
            detect_redundancies: Some(true),
            find_unreachable: Some(true),
            redundancy_threshold: Some(0.8),
            include_explanations: Some(true),
            timeout_ms: Some(30000), // 30 seconds default timeout
        }
    }
}

impl Default for ConflictAnalysisMetrics {
    fn default() -> Self {
        Self {
            total_duration_ms: 0,
            conflict_detection_ms: 0,
            redundancy_analysis_ms: 0,
            reachability_analysis_ms: 0,
            memory_usage_bytes: None,
            combinations_analyzed: 0,
        }
    }
}

impl PolicyReference {
    pub fn new(id: String) -> Self {
        Self {
            id,
            name: None,
            line: None,
            column: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_location(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_policy_conflicts_request_creation() {
        let policy1 = PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string());
        let policy2 = PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string());
        
        let request = DetectPolicyConflictsRequest::new(vec![policy1, policy2]);
        assert_eq!(request.policies.len(), 2);
        assert!(request.options.is_none());
        assert!(request.context.is_none());
    }

    #[test]
    fn test_policy_for_analysis_builder() {
        let policy = PolicyForAnalysis::new("test-policy".to_string(), "permit(principal, action, resource);".to_string())
            .with_name("Test Policy".to_string())
            .with_priority(10);
        
        assert_eq!(policy.id, "test-policy");
        assert_eq!(policy.name, Some("Test Policy".to_string()));
        assert_eq!(policy.priority, Some(10));
    }

    #[test]
    fn test_conflict_response_no_conflicts() {
        let metrics = ConflictAnalysisMetrics::default();
        let response = DetectPolicyConflictsResponse::no_conflicts(metrics);
        
        assert!(!response.has_conflicts);
        assert!(response.conflict_analysis.conflicts.is_empty());
        assert_eq!(response.get_conflict_summary(), "No conflicts detected");
    }

    #[test]
    fn test_conflict_analysis_options_default() {
        let options = ConflictAnalysisOptions::default();
        assert_eq!(options.detect_redundancies, Some(true));
        assert_eq!(options.find_unreachable, Some(true));
        assert_eq!(options.redundancy_threshold, Some(0.8));
        assert_eq!(options.include_explanations, Some(true));
        assert_eq!(options.timeout_ms, Some(30000));
    }

    #[test]
    fn test_policy_reference_builder() {
        let policy_ref = PolicyReference::new("policy-123".to_string())
            .with_name("My Policy".to_string())
            .with_location(10, 5);
        
        assert_eq!(policy_ref.id, "policy-123");
        assert_eq!(policy_ref.name, Some("My Policy".to_string()));
        assert_eq!(policy_ref.line, Some(10));
        assert_eq!(policy_ref.column, Some(5));
    }

    #[test]
    fn test_conflict_severity_ordering() {
        assert!(ConflictSeverity::Critical != ConflictSeverity::High);
        assert!(ConflictSeverity::High != ConflictSeverity::Medium);
        assert!(ConflictSeverity::Medium != ConflictSeverity::Low);
        assert!(ConflictSeverity::Low != ConflictSeverity::Info);
    }

    #[test]
    fn test_conflict_type_variants() {
        let conflict_types = vec![
            ConflictType::DirectContradiction,
            ConflictType::OverlappingPermissions,
            ConflictType::AmbiguousPrecedence,
            ConflictType::CircularDependency,
            ConflictType::InconsistentConditions,
            ConflictType::ResourceAccessConflict,
            ConflictType::ActionPermissionConflict,
        ];
        
        assert_eq!(conflict_types.len(), 7);
    }
}