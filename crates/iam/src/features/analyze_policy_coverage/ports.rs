use async_trait::async_trait;
use std::collections::HashMap;
use crate::domain::policy::{Policy, PolicyId};
use crate::infrastructure::errors::IamError;
use super::dto::*;

/// Port for analyzing policy coverage against schema
#[async_trait]
pub trait CoverageAnalysisPort: Send + Sync {
    async fn analyze_coverage(
        &self,
        policies: &[Policy],
        schema_version: Option<&str>,
    ) -> Result<CoverageReport, IamError>;
}

/// Port for identifying coverage gaps
#[async_trait]
pub trait CoverageGapDetectionPort: Send + Sync {
    async fn detect_gaps(
        &self,
        coverage_report: &CoverageReport,
        policies: &[Policy],
    ) -> Result<Vec<CoverageGap>, IamError>;
}

/// Port for generating coverage suggestions
#[async_trait]
pub trait CoverageSuggestionPort: Send + Sync {
    async fn generate_suggestions(
        &self,
        gaps: &[CoverageGap],
        existing_policies: &[Policy],
    ) -> Result<Vec<CoverageSuggestion>, IamError>;
}

/// Port for schema analysis
#[async_trait]
pub trait SchemaAnalysisPort: Send + Sync {
    async fn get_schema_entities(&self) -> Result<HashMap<String, Vec<String>>, IamError>;
    async fn get_schema_actions(&self) -> Result<Vec<String>, IamError>;
    async fn validate_schema_version(&self, version: Option<&str>) -> Result<String, IamError>;
}

/// Port for policy repository access
#[async_trait]
pub trait PolicyCoverageRepositoryPort: Send + Sync {
    async fn get_policies_by_ids(&self, ids: &[PolicyId]) -> Result<Vec<Policy>, IamError>;
    async fn get_all_active_policies(&self) -> Result<Vec<Policy>, IamError>;
}

/// Port for coverage metrics collection
#[async_trait]
pub trait CoverageMetricsPort: Send + Sync {
    async fn record_analysis_duration(&self, duration_ms: u64);
    async fn record_coverage_percentage(&self, percentage: f64);
    async fn record_gaps_found(&self, gap_count: usize);
    async fn record_suggestions_generated(&self, suggestion_count: usize);
}