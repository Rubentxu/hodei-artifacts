use std::sync::Arc;
use std::collections::HashMap;
use async_trait::async_trait;
use crate::domain::policy::{Policy, PolicyId, PolicyStatus};
use crate::infrastructure::errors::IamError;
use super::dto::*;
use super::ports::*;
use super::use_case::AnalyzePolicyCoverageUseCase;

// Mock implementations for testing
pub struct MockCoverageAnalysisPort {
    pub should_fail: bool,
}

#[async_trait]
impl CoverageAnalysisPort for MockCoverageAnalysisPort {
    async fn analyze_coverage(
        &self,
        policies: &[Policy],
        _schema_version: Option<&str>,
    ) -> Result<CoverageReport, IamError> {
        if self.should_fail {
            return Err(IamError::ValidationFailed("Mock analysis failure".to_string()));
        }

        let mut report = CoverageReport::new();
        report.total_entities = 2;
        report.covered_entities = 1;
        report.total_actions = 3;
        report.covered_actions = 2;
        report.calculate_coverage_percentage();

        // Add mock entity coverage
        report.entity_coverage.insert(
            "User".to_string(),
            EntityCoverage {
                entity_type: "User".to_string(),
                total_attributes: 3,
                covered_attributes: 3,
                coverage_percentage: 100.0,
                missing_attributes: vec![],
            },
        );

        report.entity_coverage.insert(
            "Artifact".to_string(),
            EntityCoverage {
                entity_type: "Artifact".to_string(),
                total_attributes: 4,
                covered_attributes: 2,
                coverage_percentage: 50.0,
                missing_attributes: vec!["type".to_string(), "size".to_string()],
            },
        );

        Ok(report)
    }
}

pub struct MockGapDetectionPort {
    pub gaps_to_return: Vec<CoverageGap>,
}

#[async_trait]
impl CoverageGapDetectionPort for MockGapDetectionPort {
    async fn detect_gaps(
        &self,
        _coverage_report: &CoverageReport,
        _policies: &[Policy],
    ) -> Result<Vec<CoverageGap>, IamError> {
        Ok(self.gaps_to_return.clone())
    }
}

pub struct MockSuggestionPort {
    pub suggestions_to_return: Vec<CoverageSuggestion>,
}

#[async_trait]
impl CoverageSuggestionPort for MockSuggestionPort {
    async fn generate_suggestions(
        &self,
        _gaps: &[CoverageGap],
        _existing_policies: &[Policy],
    ) -> Result<Vec<CoverageSuggestion>, IamError> {
        Ok(self.suggestions_to_return.clone())
    }
}

pub struct MockSchemaAnalysisPort;

#[async_trait]
impl SchemaAnalysisPort for MockSchemaAnalysisPort {
    async fn get_schema_entities(&self) -> Result<HashMap<String, Vec<String>>, IamError> {
        let mut entities = HashMap::new();
        entities.insert("User".to_string(), vec!["id".to_string(), "email".to_string()]);
        entities.insert("Artifact".to_string(), vec!["id".to_string(), "name".to_string()]);
        Ok(entities)
    }

    async fn get_schema_actions(&self) -> Result<Vec<String>, IamError> {
        Ok(vec!["read".to_string(), "write".to_string(), "delete".to_string()])
    }

    async fn validate_schema_version(&self, version: Option<&str>) -> Result<String, IamError> {
        Ok(version.unwrap_or("1.0.0").to_string())
    }
}

pub struct MockRepositoryPort {
    pub policies: Vec<Policy>,
    pub should_fail: bool,
}

#[async_trait]
impl PolicyCoverageRepositoryPort for MockRepositoryPort {
    async fn get_policies_by_ids(&self, ids: &[PolicyId]) -> Result<Vec<Policy>, IamError> {
        if self.should_fail {
            return Err(IamError::DatabaseError("Mock repository failure".to_string()));
        }

        let policies: Vec<Policy> = self.policies
            .iter()
            .filter(|p| ids.contains(&p.id))
            .cloned()
            .collect();

        if policies.len() != ids.len() {
            return Err(IamError::PolicyNotFound("Some policies not found".to_string()));
        }

        Ok(policies)
    }

    async fn get_all_active_policies(&self) -> Result<Vec<Policy>, IamError> {
        if self.should_fail {
            return Err(IamError::DatabaseError("Mock repository failure".to_string()));
        }

        Ok(self.policies
            .iter()
            .filter(|p| p.status == PolicyStatus::Active)
            .cloned()
            .collect())
    }
}

pub struct MockMetricsPort {
    pub recorded_metrics: Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl MockMetricsPort {
    pub fn new() -> Self {
        Self {
            recorded_metrics: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl CoverageMetricsPort for MockMetricsPort {
    async fn record_analysis_duration(&self, duration_ms: u64) {
        let mut metrics = self.recorded_metrics.lock().await;
        metrics.push(format!("analysis_duration: {}", duration_ms));
    }

    async fn record_coverage_percentage(&self, percentage: f64) {
        let mut metrics = self.recorded_metrics.lock().await;
        metrics.push(format!("coverage_percentage: {}", percentage));
    }

    async fn record_gaps_found(&self, gap_count: usize) {
        let mut metrics = self.recorded_metrics.lock().await;
        metrics.push(format!("gaps_found: {}", gap_count));
    }

    async fn record_suggestions_generated(&self, suggestion_count: usize) {
        let mut metrics = self.recorded_metrics.lock().await;
        metrics.push(format!("suggestions_generated: {}", suggestion_count));
    }
}

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
async fn test_analyze_coverage_success() {
    let coverage_port = Arc::new(MockCoverageAnalysisPort { should_fail: false });
    let gap_port = Arc::new(MockGapDetectionPort { 
        gaps_to_return: vec![
            CoverageGap {
                gap_type: CoverageGapType::MissingAttribute,
                entity_type: Some("Artifact".to_string()),
                action_name: None,
                attribute_name: Some("type".to_string()),
                description: "Missing attribute coverage".to_string(),
                severity: GapSeverity::Medium,
            }
        ]
    });
    let suggestion_port = Arc::new(MockSuggestionPort { 
        suggestions_to_return: vec![
            CoverageSuggestion {
                suggestion_type: SuggestionType::AddAttribute,
                target_gap: CoverageGap {
                    gap_type: CoverageGapType::MissingAttribute,
                    entity_type: Some("Artifact".to_string()),
                    action_name: None,
                    attribute_name: Some("type".to_string()),
                    description: "Missing attribute coverage".to_string(),
                    severity: GapSeverity::Medium,
                },
                recommended_action: "Add type attribute coverage".to_string(),
                policy_template: None,
                priority: SuggestionPriority::Medium,
            }
        ]
    });
    let schema_port = Arc::new(MockSchemaAnalysisPort);
    let repository_port = Arc::new(MockRepositoryPort {
        policies: vec![
            create_test_policy("1", "permit (principal, action, resource);"),
            create_test_policy("2", "deny (principal, action, resource);"),
        ],
        should_fail: false,
    });
    let metrics_port = Arc::new(MockMetricsPort::new());

    let use_case = AnalyzePolicyCoverageUseCase::new(
        coverage_port,
        gap_port,
        suggestion_port,
        schema_port,
        repository_port,
        metrics_port.clone(),
    );

    let request = AnalyzeCoverageRequest {
        policies: vec![],
        schema_version: Some("1.0.0".to_string()),
        include_suggestions: true,
    };

    let result = use_case.execute(request).await;
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.coverage_report.total_entities, 2);
    assert_eq!(response.coverage_report.covered_entities, 1);
    assert_eq!(response.gaps.len(), 1);
    assert_eq!(response.suggestions.len(), 1);
    assert_eq!(response.analysis_metadata.policies_analyzed, 2);

    // Verify metrics were recorded
    let recorded_metrics = metrics_port.recorded_metrics.lock().await;
    assert!(recorded_metrics.len() >= 4);
}

#[tokio::test]
async fn test_analyze_coverage_no_policies() {
    let coverage_port = Arc::new(MockCoverageAnalysisPort { should_fail: false });
    let gap_port = Arc::new(MockGapDetectionPort { gaps_to_return: vec![] });
    let suggestion_port = Arc::new(MockSuggestionPort { suggestions_to_return: vec![] });
    let schema_port = Arc::new(MockSchemaAnalysisPort);
    let repository_port = Arc::new(MockRepositoryPort {
        policies: vec![],
        should_fail: false,
    });
    let metrics_port = Arc::new(MockMetricsPort::new());

    let use_case = AnalyzePolicyCoverageUseCase::new(
        coverage_port,
        gap_port,
        suggestion_port,
        schema_port,
        repository_port,
        metrics_port,
    );

    let request = AnalyzeCoverageRequest::default();

    let result = use_case.execute(request).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        IamError::ValidationFailed(msg) => {
            assert!(msg.contains("No policies found"));
        }
        _ => panic!("Expected ValidationFailed error"),
    }
}

#[tokio::test]
async fn test_analyze_coverage_repository_failure() {
    let coverage_port = Arc::new(MockCoverageAnalysisPort { should_fail: false });
    let gap_port = Arc::new(MockGapDetectionPort { gaps_to_return: vec![] });
    let suggestion_port = Arc::new(MockSuggestionPort { suggestions_to_return: vec![] });
    let schema_port = Arc::new(MockSchemaAnalysisPort);
    let repository_port = Arc::new(MockRepositoryPort {
        policies: vec![],
        should_fail: true,
    });
    let metrics_port = Arc::new(MockMetricsPort::new());

    let use_case = AnalyzePolicyCoverageUseCase::new(
        coverage_port,
        gap_port,
        suggestion_port,
        schema_port,
        repository_port,
        metrics_port,
    );

    let request = AnalyzeCoverageRequest::default();

    let result = use_case.execute(request).await;
    assert!(result.is_err());
    
    match result.unwrap_err() {
        IamError::DatabaseError(_) => {
            // Expected error type
        }
        _ => panic!("Expected DatabaseError"),
    }
}

#[tokio::test]
async fn test_analyze_coverage_without_suggestions() {
    let coverage_port = Arc::new(MockCoverageAnalysisPort { should_fail: false });
    let gap_port = Arc::new(MockGapDetectionPort { 
        gaps_to_return: vec![
            CoverageGap {
                gap_type: CoverageGapType::UncoveredAction,
                entity_type: None,
                action_name: Some("delete".to_string()),
                attribute_name: None,
                description: "Action not covered".to_string(),
                severity: GapSeverity::High,
            }
        ]
    });
    let suggestion_port = Arc::new(MockSuggestionPort { suggestions_to_return: vec![] });
    let schema_port = Arc::new(MockSchemaAnalysisPort);
    let repository_port = Arc::new(MockRepositoryPort {
        policies: vec![create_test_policy("1", "permit (principal, action, resource);")],
        should_fail: false,
    });
    let metrics_port = Arc::new(MockMetricsPort::new());

    let use_case = AnalyzePolicyCoverageUseCase::new(
        coverage_port,
        gap_port,
        suggestion_port,
        schema_port,
        repository_port,
        metrics_port,
    );

    let request = AnalyzeCoverageRequest {
        policies: vec![],
        schema_version: None,
        include_suggestions: false,
    };

    let result = use_case.execute(request).await;
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.gaps.len(), 1);
    assert_eq!(response.suggestions.len(), 0); // No suggestions requested
}

#[tokio::test]
async fn test_analyze_coverage_specific_policies() {
    let policy_id = PolicyId::from_string("specific-policy".to_string());
    let coverage_port = Arc::new(MockCoverageAnalysisPort { should_fail: false });
    let gap_port = Arc::new(MockGapDetectionPort { gaps_to_return: vec![] });
    let suggestion_port = Arc::new(MockSuggestionPort { suggestions_to_return: vec![] });
    let schema_port = Arc::new(MockSchemaAnalysisPort);
    let repository_port = Arc::new(MockRepositoryPort {
        policies: vec![
            Policy {
                id: policy_id.clone(),
                name: "Specific Policy".to_string(),
                content: "permit (principal, action, resource);".to_string(),
                status: PolicyStatus::Active,
                created_by: "test_user".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                tags: vec![],
                description: Some("Specific test policy".to_string()),
            }
        ],
        should_fail: false,
    });
    let metrics_port = Arc::new(MockMetricsPort::new());

    let use_case = AnalyzePolicyCoverageUseCase::new(
        coverage_port,
        gap_port,
        suggestion_port,
        schema_port,
        repository_port,
        metrics_port,
    );

    let request = AnalyzeCoverageRequest {
        policies: vec![policy_id],
        schema_version: None,
        include_suggestions: true,
    };

    let result = use_case.execute(request).await;
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.analysis_metadata.policies_analyzed, 1);
}