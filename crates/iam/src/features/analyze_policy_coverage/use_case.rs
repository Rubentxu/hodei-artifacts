use async_trait::async_trait;
use std::sync::Arc;
use chrono::Utc;
use crate::infrastructure::errors::IamError;
use super::dto::*;
use super::ports::*;

pub struct AnalyzePolicyCoverageUseCase {
    coverage_analysis_port: Arc<dyn CoverageAnalysisPort>,
    gap_detection_port: Arc<dyn CoverageGapDetectionPort>,
    suggestion_port: Arc<dyn CoverageSuggestionPort>,
    schema_analysis_port: Arc<dyn SchemaAnalysisPort>,
    repository_port: Arc<dyn PolicyCoverageRepositoryPort>,
    metrics_port: Arc<dyn CoverageMetricsPort>,
}

impl AnalyzePolicyCoverageUseCase {
    pub fn new(
        coverage_analysis_port: Arc<dyn CoverageAnalysisPort>,
        gap_detection_port: Arc<dyn CoverageGapDetectionPort>,
        suggestion_port: Arc<dyn CoverageSuggestionPort>,
        schema_analysis_port: Arc<dyn SchemaAnalysisPort>,
        repository_port: Arc<dyn PolicyCoverageRepositoryPort>,
        metrics_port: Arc<dyn CoverageMetricsPort>,
    ) -> Self {
        Self {
            coverage_analysis_port,
            gap_detection_port,
            suggestion_port,
            schema_analysis_port,
            repository_port,
            metrics_port,
        }
    }

    pub async fn execute(&self, request: AnalyzeCoverageRequest) -> Result<AnalyzeCoverageResponse, IamError> {
        let start_time = std::time::Instant::now();
        
        // Validate schema version
        let schema_version = self.schema_analysis_port
            .validate_schema_version(request.schema_version.as_deref())
            .await?;

        // Get policies to analyze
        let policies = if request.policies.is_empty() {
            self.repository_port.get_all_active_policies().await?
        } else {
            self.repository_port.get_policies_by_ids(&request.policies).await?
        };

        if policies.is_empty() {
            return Err(IamError::ValidationFailed("No policies found for analysis".to_string()));
        }

        // Perform coverage analysis
        let mut coverage_report = self.coverage_analysis_port
            .analyze_coverage(&policies, Some(&schema_version))
            .await?;

        coverage_report.calculate_coverage_percentage();

        // Detect coverage gaps
        let gaps = self.gap_detection_port
            .detect_gaps(&coverage_report, &policies)
            .await?;

        // Generate suggestions if requested
        let suggestions = if request.include_suggestions {
            self.suggestion_port
                .generate_suggestions(&gaps, &policies)
                .await?
        } else {
            Vec::new()
        };

        let analysis_duration = start_time.elapsed().as_millis() as u64;

        // Record metrics
        self.metrics_port.record_analysis_duration(analysis_duration).await;
        self.metrics_port.record_coverage_percentage(coverage_report.coverage_percentage).await;
        self.metrics_port.record_gaps_found(gaps.len()).await;
        self.metrics_port.record_suggestions_generated(suggestions.len()).await;

        let analysis_metadata = AnalysisMetadata {
            analysis_timestamp: Utc::now(),
            schema_version,
            policies_analyzed: policies.len(),
            analysis_duration_ms: analysis_duration,
        };

        Ok(AnalyzeCoverageResponse {
            coverage_report,
            gaps,
            suggestions,
            analysis_metadata,
        })
    }
}

#[async_trait]
pub trait AnalyzePolicyCoverageUseCasePort: Send + Sync {
    async fn execute(&self, request: AnalyzeCoverageRequest) -> Result<AnalyzeCoverageResponse, IamError>;
}

#[async_trait]
impl AnalyzePolicyCoverageUseCasePort for AnalyzePolicyCoverageUseCase {
    async fn execute(&self, request: AnalyzeCoverageRequest) -> Result<AnalyzeCoverageResponse, IamError> {
        self.execute(request).await
    }
}