use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::domain::policy::PolicyId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeCoverageRequest {
    pub policies: Vec<PolicyId>,
    pub schema_version: Option<String>,
    pub include_suggestions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeCoverageResponse {
    pub coverage_report: CoverageReport,
    pub gaps: Vec<CoverageGap>,
    pub suggestions: Vec<CoverageSuggestion>,
    pub analysis_metadata: AnalysisMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub total_entities: usize,
    pub covered_entities: usize,
    pub total_actions: usize,
    pub covered_actions: usize,
    pub coverage_percentage: f64,
    pub entity_coverage: HashMap<String, EntityCoverage>,
    pub action_coverage: HashMap<String, ActionCoverage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCoverage {
    pub entity_type: String,
    pub total_attributes: usize,
    pub covered_attributes: usize,
    pub coverage_percentage: f64,
    pub missing_attributes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCoverage {
    pub action_name: String,
    pub is_covered: bool,
    pub covering_policies: Vec<PolicyId>,
    pub context_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageGap {
    pub gap_type: CoverageGapType,
    pub entity_type: Option<String>,
    pub action_name: Option<String>,
    pub attribute_name: Option<String>,
    pub description: String,
    pub severity: GapSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoverageGapType {
    UncoveredEntity,
    UncoveredAction,
    MissingAttribute,
    InsufficientPermissions,
    OverlyPermissive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSuggestion {
    pub suggestion_type: SuggestionType,
    pub target_gap: CoverageGap,
    pub recommended_action: String,
    pub policy_template: Option<String>,
    pub priority: SuggestionPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionType {
    CreatePolicy,
    ModifyExistingPolicy,
    AddAttribute,
    RefinePermissions,
    AddContextConstraint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionPriority {
    Immediate,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisMetadata {
    pub analysis_timestamp: chrono::DateTime<chrono::Utc>,
    pub schema_version: String,
    pub policies_analyzed: usize,
    pub analysis_duration_ms: u64,
}

impl Default for AnalyzeCoverageRequest {
    fn default() -> Self {
        Self {
            policies: Vec::new(),
            schema_version: None,
            include_suggestions: true,
        }
    }
}

impl CoverageReport {
    pub fn new() -> Self {
        Self {
            total_entities: 0,
            covered_entities: 0,
            total_actions: 0,
            covered_actions: 0,
            coverage_percentage: 0.0,
            entity_coverage: HashMap::new(),
            action_coverage: HashMap::new(),
        }
    }

    pub fn calculate_coverage_percentage(&mut self) {
        let total_items = self.total_entities + self.total_actions;
        let covered_items = self.covered_entities + self.covered_actions;
        
        self.coverage_percentage = if total_items > 0 {
            (covered_items as f64 / total_items as f64) * 100.0
        } else {
            0.0
        };
    }
}