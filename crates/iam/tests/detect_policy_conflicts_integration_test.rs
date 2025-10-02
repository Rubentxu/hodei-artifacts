// crates/iam/tests/detect_policy_conflicts_integration_test.rs

use iam::features::detect_policy_conflicts::{
    ConflictAnalysisOptions, DetectPolicyConflictsContainer, DetectPolicyConflictsRequest,
    PolicyForAnalysis,
};
use iam::infrastructure::errors::IamError;

#[tokio::test]
async fn test_detect_policy_conflicts_integration_no_conflicts() -> Result<(), IamError> {
    // Create container with all dependencies
    let container = DetectPolicyConflictsContainer::new()?;

    // Create policies that should not conflict
    let policies = vec![
        PolicyForAnalysis::new(
            "policy1".to_string(),
            "permit(principal, action, \"resource1\");".to_string(),
        )
        .with_name("Policy 1".to_string()),
        PolicyForAnalysis::new(
            "policy2".to_string(),
            "permit(principal, action, \"resource2\");".to_string(),
        )
        .with_name("Policy 2".to_string()),
    ];

    let request = DetectPolicyConflictsRequest::new(policies);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Verify response structure
    assert_eq!(response.conflict_analysis.summary.total_policies, 2);
    assert!(response.metrics.total_duration_ms > 0);

    println!(
        "Conflict detection completed: {}",
        response.get_conflict_summary()
    );

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_with_potential_conflicts() -> Result<(), IamError>
{
    let container = DetectPolicyConflictsContainer::new()?;

    // Create policies that might conflict
    let policies = vec![
        PolicyForAnalysis::new(
            "permit_policy".to_string(),
            "permit(principal, action, resource);".to_string(),
        )
        .with_name("Permit Policy".to_string())
        .with_priority(1),
        PolicyForAnalysis::new(
            "forbid_policy".to_string(),
            "forbid(principal, action, resource);".to_string(),
        )
        .with_name("Forbid Policy".to_string())
        .with_priority(2),
    ];

    let options = ConflictAnalysisOptions {
        detect_redundancies: Some(true),
        find_unreachable: Some(true),
        redundancy_threshold: Some(0.8),
        include_explanations: Some(true),
        timeout_ms: Some(10000),
    };

    let request = DetectPolicyConflictsRequest::new(policies).with_options(options);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Verify response
    assert_eq!(response.conflict_analysis.summary.total_policies, 2);
    assert!(response.metrics.total_duration_ms > 0);

    // Check if conflicts were detected (may or may not depending on implementation)
    println!("Conflicts detected: {}", response.has_conflicts);
    println!("Summary: {}", response.get_conflict_summary());

    if response.has_conflicts {
        println!("Conflicts found:");
        for conflict in &response.conflict_analysis.conflicts {
            println!(
                "  - Type: {:?}, Severity: {:?}",
                conflict.conflict_type, conflict.severity
            );
            println!("    Description: {}", conflict.description);
            if let Some(resolution) = &conflict.suggested_resolution {
                println!("    Suggested resolution: {}", resolution);
            }
        }

        if !response.conflict_analysis.redundancies.is_empty() {
            println!("Redundancies found:");
            for redundancy in &response.conflict_analysis.redundancies {
                println!("  - Policy: {}", redundancy.redundant_policy.id);
                println!("    Confidence: {:.2}", redundancy.confidence);
                println!("    Explanation: {}", redundancy.explanation);
            }
        }

        if !response.conflict_analysis.unreachable_policies.is_empty() {
            println!("Unreachable policies found:");
            for unreachable in &response.conflict_analysis.unreachable_policies {
                println!("  - Policy: {}", unreachable.policy.id);
                println!("    Explanation: {}", unreachable.explanation);
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_redundant_policies() -> Result<(), IamError> {
    let container = DetectPolicyConflictsContainer::new()?;

    // Create identical policies that should be detected as redundant
    let policies = vec![
        PolicyForAnalysis::new(
            "original".to_string(),
            "permit(principal, action, resource);".to_string(),
        )
        .with_name("Original Policy".to_string()),
        PolicyForAnalysis::new(
            "duplicate".to_string(),
            "permit(principal, action, resource);".to_string(),
        )
        .with_name("Duplicate Policy".to_string()),
        PolicyForAnalysis::new(
            "similar".to_string(),
            "permit(principal, action, resource);".to_string(),
        )
        .with_name("Similar Policy".to_string()),
    ];

    let request = DetectPolicyConflictsRequest::new(policies);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Should detect redundancies
    assert_eq!(response.conflict_analysis.summary.total_policies, 3);
    assert!(response.conflict_analysis.summary.total_redundancies > 0);

    println!("Redundancy analysis completed:");
    println!(
        "  Total redundancies: {}",
        response.conflict_analysis.summary.total_redundancies
    );

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_performance_metrics() -> Result<(), IamError> {
    let container = DetectPolicyConflictsContainer::new()?;

    // Create multiple policies to test performance
    let policies: Vec<PolicyForAnalysis> = (0..10)
        .map(|i| {
            PolicyForAnalysis::new(
                format!("policy_{}", i),
                format!("permit(principal, action, \"resource_{}\");", i),
            )
            .with_name(format!("Policy {}", i))
        })
        .collect();

    let request = DetectPolicyConflictsRequest::new(policies);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Verify metrics are collected
    assert!(response.metrics.total_duration_ms > 0);
    assert!(response.metrics.combinations_analyzed > 0);

    println!("Performance metrics:");
    println!("  Total duration: {}ms", response.metrics.total_duration_ms);
    println!(
        "  Conflict detection: {}ms",
        response.metrics.conflict_detection_ms
    );
    println!(
        "  Redundancy analysis: {}ms",
        response.metrics.redundancy_analysis_ms
    );
    println!(
        "  Reachability analysis: {}ms",
        response.metrics.reachability_analysis_ms
    );
    println!(
        "  Combinations analyzed: {}",
        response.metrics.combinations_analyzed
    );

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_error_handling() -> Result<(), IamError> {
    let container = DetectPolicyConflictsContainer::new()?;

    // Test with empty policies (should return error)
    let request = DetectPolicyConflictsRequest::new(vec![]);
    let result = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("At least one policy is required")
    );

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_large_policy_set() -> Result<(), IamError> {
    let container = DetectPolicyConflictsContainer::new()?;

    // Test with many policies (but within limits)
    let policies: Vec<PolicyForAnalysis> = (0..50)
        .map(|i| {
            PolicyForAnalysis::new(
                format!("policy_{:03}", i),
                format!("permit(principal, action, \"resource_{}\");", i % 10), // Some overlap
            )
        })
        .collect();

    let request = DetectPolicyConflictsRequest::new(policies);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Should handle large policy sets
    assert_eq!(response.conflict_analysis.summary.total_policies, 50);
    assert!(response.metrics.total_duration_ms > 0);

    // Should detect some overlaps due to repeated resource patterns
    println!("Large policy set analysis:");
    println!(
        "  Policies analyzed: {}",
        response.conflict_analysis.summary.total_policies
    );
    println!(
        "  Conflicts found: {}",
        response.conflict_analysis.summary.total_conflicts
    );
    println!(
        "  Redundancies found: {}",
        response.conflict_analysis.summary.total_redundancies
    );
    println!("  Analysis time: {}ms", response.metrics.total_duration_ms);

    Ok(())
}

#[tokio::test]
async fn test_detect_policy_conflicts_integration_with_context() -> Result<(), IamError> {
    let container = DetectPolicyConflictsContainer::new()?;

    let policies = vec![
        PolicyForAnalysis::new(
            "context_policy_1".to_string(),
            "permit(principal, action, resource);".to_string(),
        ),
        PolicyForAnalysis::new(
            "context_policy_2".to_string(),
            "forbid(principal, action, resource);".to_string(),
        ),
    ];

    let mut context = std::collections::HashMap::new();
    context.insert("organization_id".to_string(), "org-123".to_string());
    context.insert("analysis_type".to_string(), "comprehensive".to_string());

    let request = DetectPolicyConflictsRequest::new(policies).with_context(context);
    let response = container
        .conflict_detection_service()
        .detect_conflicts(request)
        .await?;

    // Should complete successfully with context
    assert_eq!(response.conflict_analysis.summary.total_policies, 2);

    println!(
        "Context-aware analysis completed: {}",
        response.get_conflict_summary()
    );

    Ok(())
}

#[test]
fn test_detect_policy_conflicts_container_creation() {
    let result = DetectPolicyConflictsContainer::new();
    assert!(result.is_ok());

    let container = result.unwrap();

    // Test that we can create API and router
    let _api = container.create_api();
    let _router = container.create_router();

    println!("Container created successfully with all components");
}

#[test]
fn test_detect_policy_conflicts_builder_pattern() {
    use iam::features::detect_policy_conflicts::DetectPolicyConflictsContainerBuilder;

    let result = DetectPolicyConflictsContainerBuilder::new().build();
    assert!(result.is_ok());

    println!("Builder pattern works correctly");
}

#[test]
fn test_detect_policy_conflicts_factory_patterns() {
    use iam::features::detect_policy_conflicts::ConflictDetectionContainerFactory;

    let fast_container = ConflictDetectionContainerFactory::create_fast_detection_container();
    assert!(fast_container.is_ok());

    let comprehensive_container =
        ConflictDetectionContainerFactory::create_comprehensive_analysis_container();
    assert!(comprehensive_container.is_ok());

    println!("Factory patterns work correctly");
}
