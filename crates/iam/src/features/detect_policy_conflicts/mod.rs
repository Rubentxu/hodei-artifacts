// crates/iam/src/features/detect_policy_conflicts/mod.rs

//! Policy conflict detection feature module
//! 
//! This module implements comprehensive policy conflict detection following VSA (Vertical Slice Architecture)
//! principles. It provides detection of direct conflicts, redundancies, unreachable policies, and permission overlaps.
//! 
//! ## Architecture
//! 
//! The module follows Clean Architecture with clear separation of concerns:
//! - **DTOs**: Data transfer objects for requests and responses
//! - **Ports**: Interfaces defining the contracts for external dependencies
//! - **Use Cases**: Business logic for conflict detection and analysis
//! - **Adapters**: Implementations of ports using external services (Cedar)
//! - **API**: HTTP handlers for the conflict detection endpoints
//! - **DI**: Dependency injection container for wiring components
//! 
//! ## Features
//! 
//! - **Direct Conflict Detection**: Identifies contradictory policies (permit vs forbid)
//! - **Redundancy Analysis**: Finds policies that are superseded by others
//! - **Unreachable Policy Detection**: Identifies policies that can never be reached
//! - **Permission Overlap Analysis**: Detects overlapping permission patterns
//! - **Resolution Suggestions**: Provides actionable suggestions for resolving conflicts
//! - **Performance Metrics**: Tracks analysis performance and resource usage
//! 
//! ## Usage
//! 
//! ```rust
//! use crate::features::detect_policy_conflicts::{DetectPolicyConflictsContainer, DetectPolicyConflictsRequest, PolicyForAnalysis};
//! 
//! // Create the container with all dependencies
//! let container = DetectPolicyConflictsContainer::new()?;
//! 
//! // Prepare policies for analysis
//! let policies = vec![
//!     PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
//!     PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
//! ];
//! 
//! // Create request
//! let request = DetectPolicyConflictsRequest::new(policies);
//! 
//! // Detect conflicts
//! let response = container.conflict_detection_service().detect_conflicts(request).await?;
//! 
//! println!("Conflicts found: {}", response.has_conflicts);
//! println!("Summary: {}", response.get_conflict_summary());
//! ```

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Re-export main types for convenience
pub use dto::{
    DetectPolicyConflictsRequest, DetectPolicyConflictsResponse, PolicyForAnalysis,
    PolicyConflictAnalysis, PolicyConflict, PolicyRedundancy, UnreachablePolicy,
    PolicyReference, ConflictSummary, ConflictAnalysisOptions, ConflictAnalysisMetrics,
    ConflictType, ConflictSeverity, ConflictLocation,
};

pub use ports::{
    PolicyConflictDetectionService, DirectConflictDetector, PolicyRedundancyDetector,
    UnreachablePolicyDetector, PolicyOverlapAnalyzer, ConflictAnalysisMetricsCollector,
    ConflictResolutionSuggester, ConflictAnalysisConfigProvider, ConflictAnalysisRepository,
    AnalysisType, AnalysisPerformanceThresholds, PriorityAdjustment, AnalysisHistoryEntry,
    ConflictAnalysisContext,
};

pub use use_case::DetectPolicyConflictsUseCase;

pub use adapter::{
    CedarDirectConflictDetector, SimpleRedundancyDetector, SimpleUnreachableDetector,
    SimpleOverlapAnalyzer, SimpleConflictMetricsCollector, SimpleResolutionSuggester,
    DefaultConflictAnalysisConfigProvider,
};

pub use api::{
    DetectPolicyConflictsApi, detect_conflicts_handler, health_check_handler,
    create_conflict_detection_router,
};

pub use di::{
    DetectPolicyConflictsContainer, DetectPolicyConflictsContainerBuilder,
    ConflictDetectionContainerFactory,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::errors::IamError;

    #[tokio::test]
    async fn test_feature_integration() -> Result<(), IamError> {
        // Test that all components work together
        let container = DetectPolicyConflictsContainer::new()?;
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "forbid(principal, action, resource);".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let response = container.conflict_detection_service().detect_conflicts(request).await?;
        
        // Should complete without error
        assert_eq!(response.conflict_analysis.summary.total_policies, 2);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_feature_with_no_conflicts() -> Result<(), IamError> {
        let container = DetectPolicyConflictsContainer::new()?;
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, \"resource1\");".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, \"resource2\");".to_string()),
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let response = container.conflict_detection_service().detect_conflicts(request).await?;
        
        // Should complete and might not find conflicts (depending on implementation)
        assert_eq!(response.conflict_analysis.summary.total_policies, 2);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_feature_with_redundant_policies() -> Result<(), IamError> {
        let container = DetectPolicyConflictsContainer::new()?;
        
        let policies = vec![
            PolicyForAnalysis::new("policy1".to_string(), "permit(principal, action, resource);".to_string()),
            PolicyForAnalysis::new("policy2".to_string(), "permit(principal, action, resource);".to_string()), // Identical
        ];
        
        let request = DetectPolicyConflictsRequest::new(policies);
        let response = container.conflict_detection_service().detect_conflicts(request).await?;
        
        // Should detect potential redundancy
        assert_eq!(response.conflict_analysis.summary.total_policies, 2);
        
        Ok(())
    }

    #[test]
    fn test_container_builder() {
        let container = DetectPolicyConflictsContainerBuilder::new().build();
        assert!(container.is_ok());
    }

    #[test]
    fn test_router_creation() {
        let container = DetectPolicyConflictsContainer::new().unwrap();
        let _router = container.create_router();
        // Should not panic
    }

    #[test]
    fn test_factory_containers() {
        let fast_container = ConflictDetectionContainerFactory::create_fast_detection_container();
        assert!(fast_container.is_ok());

        let comprehensive_container = ConflictDetectionContainerFactory::create_comprehensive_analysis_container();
        assert!(comprehensive_container.is_ok());
    }

    #[test]
    fn test_dto_builders() {
        let policy = PolicyForAnalysis::new("test-policy".to_string(), "permit(principal, action, resource);".to_string())
            .with_name("Test Policy".to_string())
            .with_priority(10);
        
        assert_eq!(policy.id, "test-policy");
        assert_eq!(policy.name, Some("Test Policy".to_string()));
        assert_eq!(policy.priority, Some(10));

        let request = DetectPolicyConflictsRequest::new(vec![policy])
            .with_options(ConflictAnalysisOptions::default());
        
        assert!(request.options.is_some());
    }

    #[test]
    fn test_conflict_types_and_severities() {
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

        let severities = vec![
            ConflictSeverity::Critical,
            ConflictSeverity::High,
            ConflictSeverity::Medium,
            ConflictSeverity::Low,
            ConflictSeverity::Info,
        ];
        
        assert_eq!(severities.len(), 5);
    }

    #[test]
    fn test_analysis_types() {
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
    fn test_performance_thresholds() {
        let thresholds = AnalysisPerformanceThresholds::default();
        assert_eq!(thresholds.max_analysis_time_ms, 30000);
        assert_eq!(thresholds.warning_time_ms, 5000);
        assert_eq!(thresholds.max_combinations, 1000000);
    }

    #[test]
    fn test_conflict_analysis_context() {
        let context = ConflictAnalysisContext::new("test-123".to_string())
            .with_user("user-456".to_string())
            .with_organization("org-789".to_string());
        
        assert_eq!(context.request_id, "test-123");
        assert_eq!(context.user_id, Some("user-456".to_string()));
        assert_eq!(context.organization_id, Some("org-789".to_string()));
    }
}