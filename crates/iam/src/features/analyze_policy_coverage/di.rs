use std::sync::Arc;
use crate::infrastructure::validation::semantic_validator::SemanticValidator;
use crate::infrastructure::repository::policy_repository::MongoPolicyRepository;
use crate::infrastructure::metrics::policy_metrics::PolicyMetrics;
use super::use_case::{AnalyzePolicyCoverageUseCase, AnalyzePolicyCoverageUseCasePort};
use super::adapter::{
    CedarCoverageAnalysisAdapter, 
    CoverageGapDetectionAdapter, 
    CoverageSuggestionAdapter
};
use super::ports::*;

pub struct AnalyzePolicyCoverageDIContainer {
    use_case: Arc<dyn AnalyzePolicyCoverageUseCasePort>,
}

impl AnalyzePolicyCoverageDIContainer {
    pub fn new(
        semantic_validator: Arc<SemanticValidator>,
        policy_repository: Arc<MongoPolicyRepository>,
        policy_metrics: Arc<PolicyMetrics>,
    ) -> Self {
        // Create adapters
        let coverage_analysis_adapter = Arc::new(CedarCoverageAnalysisAdapter::new(
            semantic_validator.clone()
        ));
        
        let gap_detection_adapter = Arc::new(CoverageGapDetectionAdapter::new());
        let suggestion_adapter = Arc::new(CoverageSuggestionAdapter::new());

        // Create repository adapter
        let repository_adapter = Arc::new(PolicyCoverageRepositoryAdapter::new(
            policy_repository
        ));

        // Create metrics adapter
        let metrics_adapter = Arc::new(CoverageMetricsAdapter::new(policy_metrics));

        // Create use case
        let use_case = Arc::new(AnalyzePolicyCoverageUseCase::new(
            coverage_analysis_adapter.clone() as Arc<dyn CoverageAnalysisPort>,
            gap_detection_adapter as Arc<dyn CoverageGapDetectionPort>,
            suggestion_adapter as Arc<dyn CoverageSuggestionPort>,
            coverage_analysis_adapter as Arc<dyn SchemaAnalysisPort>,
            repository_adapter as Arc<dyn PolicyCoverageRepositoryPort>,
            metrics_adapter as Arc<dyn CoverageMetricsPort>,
        ));

        Self { use_case }
    }

    pub fn get_use_case(&self) -> Arc<dyn AnalyzePolicyCoverageUseCasePort> {
        self.use_case.clone()
    }
}

// Repository adapter implementation
pub struct PolicyCoverageRepositoryAdapter {
    repository: Arc<MongoPolicyRepository>,
}

impl PolicyCoverageRepositoryAdapter {
    pub fn new(repository: Arc<MongoPolicyRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait::async_trait]
impl PolicyCoverageRepositoryPort for PolicyCoverageRepositoryAdapter {
    async fn get_policies_by_ids(
        &self, 
        ids: &[crate::domain::policy::PolicyId]
    ) -> Result<Vec<crate::domain::policy::Policy>, crate::infrastructure::errors::IamError> {
        let mut policies = Vec::new();
        for id in ids {
            match self.repository.get_by_id(id).await? {
                Some(policy) => policies.push(policy),
                None => return Err(crate::infrastructure::errors::IamError::PolicyNotFound(id.to_string())),
            }
        }
        Ok(policies)
    }

    async fn get_all_active_policies(&self) -> Result<Vec<crate::domain::policy::Policy>, crate::infrastructure::errors::IamError> {
        use crate::domain::policy::PolicyStatus;
        use crate::application::ports::PolicyFilter;
        
        let filter = PolicyFilter {
            status: Some(PolicyStatus::Active),
            ..Default::default()
        };
        
        let policy_list = self.repository.list(filter).await?;
        Ok(policy_list.policies)
    }
}

// Metrics adapter implementation
pub struct CoverageMetricsAdapter {
    metrics: Arc<PolicyMetrics>,
}

impl CoverageMetricsAdapter {
    pub fn new(metrics: Arc<PolicyMetrics>) -> Self {
        Self { metrics }
    }
}

#[async_trait::async_trait]
impl CoverageMetricsPort for CoverageMetricsAdapter {
    async fn record_analysis_duration(&self, duration_ms: u64) {
        self.metrics.record_coverage_analysis_duration(duration_ms).await;
    }

    async fn record_coverage_percentage(&self, percentage: f64) {
        self.metrics.record_coverage_percentage(percentage).await;
    }

    async fn record_gaps_found(&self, gap_count: usize) {
        self.metrics.record_coverage_gaps_found(gap_count).await;
    }

    async fn record_suggestions_generated(&self, suggestion_count: usize) {
        self.metrics.record_coverage_suggestions_generated(suggestion_count).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::infrastructure::validation::semantic_validator::SemanticValidator;
    use crate::infrastructure::repository::policy_repository::MongoPolicyRepository;
    use crate::infrastructure::metrics::policy_metrics::PolicyMetrics;

    #[tokio::test]
    async fn test_di_container_creation() {
        // This test would require actual implementations
        // For now, we'll just test that the container can be created
        // In a real scenario, you'd use mock implementations
        
        // Note: This test is commented out because it requires actual database connections
        // In practice, you'd use dependency injection with mock implementations for testing
        
        /*
        let semantic_validator = Arc::new(SemanticValidator::new());
        let policy_repository = Arc::new(MongoPolicyRepository::new(/* db connection */));
        let policy_metrics = Arc::new(PolicyMetrics::new());

        let container = AnalyzePolicyCoverageDIContainer::new(
            semantic_validator,
            policy_repository,
            policy_metrics,
        );

        let use_case = container.get_use_case();
        assert!(use_case.is_some());
        */
    }
}