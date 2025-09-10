use super::adapter::*;
use super::dto::*;
use crate::domain::policy::{Policy, PolicyId, PolicyStatus};
use crate::infrastructure::validation::semantic_validator::SemanticValidator;
use std::sync::Arc;

fn create_test_policy(id: &str, content: &str) -> Policy {
    Policy {
        id: PolicyId::from_string(id.to_string()),
        name: format!("Test Policy {}", id),
        content: content.to_string(),
        status: PolicyStatus::Active,
        created_by: "test_user".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        tags: vec![],
        description: Some("Test policy".to_string()),
    }
}

#[tokio::test]
async fn test_cedar_coverage_analysis_adapter() {
    // Create a mock semantic validator
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#),
        create_test_policy("2", r#"
            permit (
                principal == User::"bob",
                action == Action::"write",
                resource == Artifact::"doc2"
            );
        "#),
    ];
    
    let coverage_report = adapter.analyze_coverage(&policies, Some("1.0.0")).await.unwrap();
    
    // Verify basic structure
    assert!(coverage_report.total_entities > 0);
    assert!(coverage_report.total_actions > 0);
    assert!(!coverage_report.entity_coverage.is_empty());
    assert!(!coverage_report.action_coverage.is_empty());
}

#[tokio::test]
async fn test_schema_analysis_port_implementation() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    // Test get_schema_entities
    let entities = adapter.get_schema_entities().await.unwrap();
    assert!(entities.contains_key("User"));
    assert!(entities.contains_key("Artifact"));
    
    let user_attributes = entities.get("User").unwrap();
    assert!(user_attributes.contains(&"id".to_string()));
    assert!(user_attributes.contains(&"email".to_string()));
    
    // Test get_schema_actions
    let actions = adapter.get_schema_actions().await.unwrap();
    assert!(actions.contains(&"read".to_string()));
    assert!(actions.contains(&"write".to_string()));
    assert!(actions.contains(&"delete".to_string()));
    
    // Test validate_schema_version
    let version = adapter.validate_schema_version(Some("2.0.0")).await.unwrap();
    assert_eq!(version, "2.0.0");
    
    let default_version = adapter.validate_schema_version(None).await.unwrap();
    assert_eq!(default_version, "1.0.0");
}

#[tokio::test]
async fn test_entity_coverage_analysis() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            ) when {
                principal.email == "alice@example.com"
            };
        "#),
    ];
    
    let mut schema_entities = std::collections::HashMap::new();
    schema_entities.insert("User".to_string(), vec!["id".to_string(), "email".to_string(), "role".to_string()]);
    schema_entities.insert("Artifact".to_string(), vec!["id".to_string(), "name".to_string(), "owner".to_string()]);
    
    let entity_coverage = adapter.analyze_entity_coverage(&policies, &schema_entities);
    
    // User entity should have some coverage (email attribute is used)
    let user_coverage = entity_coverage.get("User").unwrap();
    assert!(user_coverage.covered_attributes > 0);
    assert!(user_coverage.coverage_percentage > 0.0);
    
    // Artifact entity might have less coverage
    let artifact_coverage = entity_coverage.get("Artifact").unwrap();
    assert_eq!(artifact_coverage.entity_type, "Artifact");
}

#[tokio::test]
async fn test_action_coverage_analysis() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#),
        create_test_policy("2", r#"
            forbid (
                principal,
                action == Action::"delete",
                resource == Artifact::"doc2"
            );
        "#),
    ];
    
    let schema_actions = vec!["read".to_string(), "write".to_string(), "delete".to_string()];
    
    let action_coverage = adapter.analyze_action_coverage(&policies, &schema_actions);
    
    // Read action should be covered
    let read_coverage = action_coverage.get("read").unwrap();
    assert!(read_coverage.is_covered);
    assert!(!read_coverage.covering_policies.is_empty());
    
    // Delete action should be covered
    let delete_coverage = action_coverage.get("delete").unwrap();
    assert!(delete_coverage.is_covered);
    
    // Write action should not be covered
    let write_coverage = action_coverage.get("write").unwrap();
    assert!(!write_coverage.is_covered);
    assert!(write_coverage.covering_policies.is_empty());
}

#[tokio::test]
async fn test_coverage_gap_detection_adapter() {
    let adapter = CoverageGapDetectionAdapter::new();
    
    let mut coverage_report = CoverageReport::new();
    
    // Add entity with no coverage
    coverage_report.entity_coverage.insert(
        "UncoveredEntity".to_string(),
        EntityCoverage {
            entity_type: "UncoveredEntity".to_string(),
            total_attributes: 3,
            covered_attributes: 0,
            coverage_percentage: 0.0,
            missing_attributes: vec!["attr1".to_string(), "attr2".to_string(), "attr3".to_string()],
        },
    );
    
    // Add action with no coverage
    coverage_report.action_coverage.insert(
        "uncovered_action".to_string(),
        ActionCoverage {
            action_name: "uncovered_action".to_string(),
            is_covered: false,
            covering_policies: vec![],
            context_requirements: vec![],
        },
    );
    
    let policies = vec![];
    let gaps = adapter.detect_gaps(&coverage_report, &policies).await.unwrap();
    
    // Should detect gaps for uncovered entity and action
    assert!(!gaps.is_empty());
    
    let entity_gaps: Vec<_> = gaps.iter()
        .filter(|gap| matches!(gap.gap_type, CoverageGapType::UncoveredEntity))
        .collect();
    assert!(!entity_gaps.is_empty());
    
    let action_gaps: Vec<_> = gaps.iter()
        .filter(|gap| matches!(gap.gap_type, CoverageGapType::UncoveredAction))
        .collect();
    assert!(!action_gaps.is_empty());
    
    let attribute_gaps: Vec<_> = gaps.iter()
        .filter(|gap| matches!(gap.gap_type, CoverageGapType::MissingAttribute))
        .collect();
    assert!(!attribute_gaps.is_empty());
}

#[tokio::test]
async fn test_coverage_suggestion_adapter() {
    let adapter = CoverageSuggestionAdapter::new();
    
    let gaps = vec![
        CoverageGap {
            gap_type: CoverageGapType::UncoveredEntity,
            entity_type: Some("MissingEntity".to_string()),
            action_name: None,
            attribute_name: None,
            description: "Entity not covered".to_string(),
            severity: GapSeverity::High,
        },
        CoverageGap {
            gap_type: CoverageGapType::UncoveredAction,
            entity_type: None,
            action_name: Some("missing_action".to_string()),
            attribute_name: None,
            description: "Action not covered".to_string(),
            severity: GapSeverity::Medium,
        },
    ];
    
    let existing_policies = vec![];
    let suggestions = adapter.generate_suggestions(&gaps, &existing_policies).await.unwrap();
    
    assert_eq!(suggestions.len(), 2);
    
    // Check entity suggestion
    let entity_suggestion = suggestions.iter()
        .find(|s| matches!(s.suggestion_type, SuggestionType::CreatePolicy) && 
                  s.recommended_action.contains("MissingEntity"))
        .unwrap();
    
    assert!(entity_suggestion.policy_template.is_some());
    let template = entity_suggestion.policy_template.as_ref().unwrap();
    assert!(template.contains("MissingEntity"));
    
    // Check action suggestion
    let action_suggestion = suggestions.iter()
        .find(|s| s.recommended_action.contains("missing_action"))
        .unwrap();
    
    assert!(action_suggestion.policy_template.is_some());
    let template = action_suggestion.policy_template.as_ref().unwrap();
    assert!(template.contains("missing_action"));
}

#[tokio::test]
async fn test_find_covered_attributes() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            ) when {
                principal.email == "alice@example.com" &&
                principal.role == "admin"
            };
        "#),
    ];
    
    let attributes = vec!["id".to_string(), "email".to_string(), "role".to_string(), "department".to_string()];
    
    let covered = adapter.find_covered_attributes(&policies, "User", &attributes);
    
    // Should find email and role attributes
    assert!(covered.contains(&"email".to_string()));
    assert!(covered.contains(&"role".to_string()));
    // Should not find department (not used in policy)
    assert!(!covered.contains(&"department".to_string()));
}

#[tokio::test]
async fn test_find_policies_covering_action() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("policy1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            );
        "#),
        create_test_policy("policy2", r#"
            permit (
                principal == User::"bob",
                action == Action::"write",
                resource == Artifact::"doc2"
            );
        "#),
        create_test_policy("policy3", r#"
            permit (
                principal == User::"charlie",
                action == Action::"read",
                resource == Artifact::"doc3"
            );
        "#),
    ];
    
    let covering_read = adapter.find_policies_covering_action(&policies, "read");
    let covering_write = adapter.find_policies_covering_action(&policies, "write");
    let covering_delete = adapter.find_policies_covering_action(&policies, "delete");
    
    // Read action should be covered by policy1 and policy3
    assert_eq!(covering_read.len(), 2);
    
    // Write action should be covered by policy2
    assert_eq!(covering_write.len(), 1);
    
    // Delete action should not be covered
    assert_eq!(covering_delete.len(), 0);
}

#[tokio::test]
async fn test_coverage_report_calculation() {
    let mut coverage_report = CoverageReport::new();
    
    coverage_report.total_entities = 4;
    coverage_report.covered_entities = 3;
    coverage_report.total_actions = 6;
    coverage_report.covered_actions = 4;
    
    coverage_report.calculate_coverage_percentage();
    
    // Expected: (3 + 4) / (4 + 6) * 100 = 70%
    assert_eq!(coverage_report.coverage_percentage, 70.0);
}

#[tokio::test]
async fn test_coverage_report_with_zero_items() {
    let mut coverage_report = CoverageReport::new();
    
    coverage_report.total_entities = 0;
    coverage_report.covered_entities = 0;
    coverage_report.total_actions = 0;
    coverage_report.covered_actions = 0;
    
    coverage_report.calculate_coverage_percentage();
    
    // Should handle division by zero gracefully
    assert_eq!(coverage_report.coverage_percentage, 0.0);
}

#[test]
fn test_coverage_gap_types() {
    let gap = CoverageGap {
        gap_type: CoverageGapType::UncoveredEntity,
        entity_type: Some("TestEntity".to_string()),
        action_name: None,
        attribute_name: None,
        description: "Test gap".to_string(),
        severity: GapSeverity::High,
    };
    
    assert!(matches!(gap.gap_type, CoverageGapType::UncoveredEntity));
    assert!(matches!(gap.severity, GapSeverity::High));
    assert_eq!(gap.entity_type, Some("TestEntity".to_string()));
}

#[test]
fn test_coverage_suggestion_types() {
    let suggestion = CoverageSuggestion {
        suggestion_type: SuggestionType::CreatePolicy,
        target_gap: CoverageGap {
            gap_type: CoverageGapType::UncoveredAction,
            entity_type: None,
            action_name: Some("test_action".to_string()),
            attribute_name: None,
            description: "Test gap".to_string(),
            severity: GapSeverity::Medium,
        },
        recommended_action: "Create a policy for test_action".to_string(),
        policy_template: Some("permit (principal, action == \"test_action\", resource);".to_string()),
        priority: SuggestionPriority::High,
    };
    
    assert!(matches!(suggestion.suggestion_type, SuggestionType::CreatePolicy));
    assert!(matches!(suggestion.priority, SuggestionPriority::High));
    assert!(suggestion.policy_template.is_some());
}

#[tokio::test]
async fn test_entity_coverage_with_partial_attributes() {
    let semantic_validator = Arc::new(SemanticValidator::new());
    let adapter = CedarCoverageAnalysisAdapter::new(semantic_validator);
    
    let policies = vec![
        create_test_policy("1", r#"
            permit (
                principal == User::"alice",
                action == Action::"read",
                resource == Artifact::"doc1"
            ) when {
                principal.email == "alice@example.com"
            };
        "#),
    ];
    
    let mut schema_entities = std::collections::HashMap::new();
    schema_entities.insert("User".to_string(), vec![
        "id".to_string(), 
        "email".to_string(), 
        "role".to_string(), 
        "department".to_string(),
        "created_at".to_string()
    ]);
    
    let entity_coverage = adapter.analyze_entity_coverage(&policies, &schema_entities);
    let user_coverage = entity_coverage.get("User").unwrap();
    
    // Should have partial coverage (only email is used)
    assert!(user_coverage.coverage_percentage > 0.0);
    assert!(user_coverage.coverage_percentage < 100.0);
    assert!(user_coverage.missing_attributes.len() > 0);
    assert!(!user_coverage.missing_attributes.contains(&"email".to_string()));
}