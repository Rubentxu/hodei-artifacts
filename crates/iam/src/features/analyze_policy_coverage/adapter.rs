use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use cedar_policy::{Schema, PolicySet};
use crate::domain::policy::{Policy, PolicyId};
use crate::infrastructure::errors::IamError;
use crate::infrastructure::validation::semantic_validator::SemanticValidator;
use super::dto::*;
use super::ports::*;

pub struct CedarCoverageAnalysisAdapter {
    semantic_validator: Arc<SemanticValidator>,
}

impl CedarCoverageAnalysisAdapter {
    pub fn new(semantic_validator: Arc<SemanticValidator>) -> Self {
        Self { semantic_validator }
    }

    fn analyze_entity_coverage(
        &self,
        policies: &[Policy],
        schema_entities: &HashMap<String, Vec<String>>,
    ) -> HashMap<String, EntityCoverage> {
        let mut entity_coverage = HashMap::new();

        for (entity_type, attributes) in schema_entities {
            let covered_attributes = self.find_covered_attributes(policies, entity_type, attributes);
            let coverage_percentage = if attributes.is_empty() {
                100.0
            } else {
                (covered_attributes.len() as f64 / attributes.len() as f64) * 100.0
            };

            let missing_attributes: Vec<String> = attributes
                .iter()
                .filter(|attr| !covered_attributes.contains(*attr))
                .cloned()
                .collect();

            entity_coverage.insert(
                entity_type.clone(),
                EntityCoverage {
                    entity_type: entity_type.clone(),
                    total_attributes: attributes.len(),
                    covered_attributes: covered_attributes.len(),
                    coverage_percentage,
                    missing_attributes,
                },
            );
        }

        entity_coverage
    }

    fn find_covered_attributes(
        &self,
        policies: &[Policy],
        entity_type: &str,
        attributes: &[String],
    ) -> Vec<String> {
        let mut covered = Vec::new();

        for policy in policies {
            // Parse policy content to find attribute references
            if let Ok(policy_set) = PolicySet::from_str(&policy.content) {
                for policy_item in policy_set.policies() {
                    let policy_text = policy_item.to_string();
                    
                    // Check if policy references this entity type
                    if policy_text.contains(entity_type) {
                        for attribute in attributes {
                            if policy_text.contains(attribute) && !covered.contains(attribute) {
                                covered.push(attribute.clone());
                            }
                        }
                    }
                }
            }
        }

        covered
    }

    fn analyze_action_coverage(
        &self,
        policies: &[Policy],
        schema_actions: &[String],
    ) -> HashMap<String, ActionCoverage> {
        let mut action_coverage = HashMap::new();

        for action in schema_actions {
            let covering_policies = self.find_policies_covering_action(policies, action);
            let is_covered = !covering_policies.is_empty();

            action_coverage.insert(
                action.clone(),
                ActionCoverage {
                    action_name: action.clone(),
                    is_covered,
                    covering_policies,
                    context_requirements: Vec::new(), // TODO: Extract from policies
                },
            );
        }

        action_coverage
    }

    fn find_policies_covering_action(&self, policies: &[Policy], action: &str) -> Vec<PolicyId> {
        let mut covering_policies = Vec::new();

        for policy in policies {
            if let Ok(policy_set) = PolicySet::from_str(&policy.content) {
                for policy_item in policy_set.policies() {
                    let policy_text = policy_item.to_string();
                    if policy_text.contains(action) {
                        covering_policies.push(policy.id.clone());
                        break;
                    }
                }
            }
        }

        covering_policies
    }
}

#[async_trait]
impl CoverageAnalysisPort for CedarCoverageAnalysisAdapter {
    async fn analyze_coverage(
        &self,
        policies: &[Policy],
        schema_version: Option<&str>,
    ) -> Result<CoverageReport, IamError> {
        // Get schema information
        let schema_entities = self.get_schema_entities().await?;
        let schema_actions = self.get_schema_actions().await?;

        // Analyze entity coverage
        let entity_coverage = self.analyze_entity_coverage(policies, &schema_entities);
        
        // Analyze action coverage
        let action_coverage = self.analyze_action_coverage(policies, &schema_actions);

        // Calculate totals
        let total_entities = schema_entities.len();
        let covered_entities = entity_coverage.values()
            .filter(|ec| ec.coverage_percentage > 0.0)
            .count();

        let total_actions = schema_actions.len();
        let covered_actions = action_coverage.values()
            .filter(|ac| ac.is_covered)
            .count();

        let mut coverage_report = CoverageReport {
            total_entities,
            covered_entities,
            total_actions,
            covered_actions,
            coverage_percentage: 0.0,
            entity_coverage,
            action_coverage,
        };

        coverage_report.calculate_coverage_percentage();

        Ok(coverage_report)
    }
}

#[async_trait]
impl SchemaAnalysisPort for CedarCoverageAnalysisAdapter {
    async fn get_schema_entities(&self) -> Result<HashMap<String, Vec<String>>, IamError> {
        // TODO: Extract from Cedar schema
        let mut entities = HashMap::new();
        entities.insert("User".to_string(), vec!["id".to_string(), "email".to_string(), "role".to_string()]);
        entities.insert("Artifact".to_string(), vec!["id".to_string(), "name".to_string(), "owner".to_string(), "type".to_string()]);
        Ok(entities)
    }

    async fn get_schema_actions(&self) -> Result<Vec<String>, IamError> {
        // TODO: Extract from Cedar schema
        Ok(vec![
            "read".to_string(),
            "write".to_string(),
            "delete".to_string(),
            "upload".to_string(),
            "download".to_string(),
        ])
    }

    async fn validate_schema_version(&self, version: Option<&str>) -> Result<String, IamError> {
        Ok(version.unwrap_or("1.0.0").to_string())
    }
}

pub struct CoverageGapDetectionAdapter;

impl CoverageGapDetectionAdapter {
    pub fn new() -> Self {
        Self
    }

    fn detect_entity_gaps(&self, coverage_report: &CoverageReport) -> Vec<CoverageGap> {
        let mut gaps = Vec::new();

        for (entity_type, coverage) in &coverage_report.entity_coverage {
            if coverage.coverage_percentage == 0.0 {
                gaps.push(CoverageGap {
                    gap_type: CoverageGapType::UncoveredEntity,
                    entity_type: Some(entity_type.clone()),
                    action_name: None,
                    attribute_name: None,
                    description: format!("Entity type '{}' has no policy coverage", entity_type),
                    severity: GapSeverity::High,
                });
            }

            for missing_attr in &coverage.missing_attributes {
                gaps.push(CoverageGap {
                    gap_type: CoverageGapType::MissingAttribute,
                    entity_type: Some(entity_type.clone()),
                    action_name: None,
                    attribute_name: Some(missing_attr.clone()),
                    description: format!("Attribute '{}' of entity '{}' is not covered by any policy", missing_attr, entity_type),
                    severity: GapSeverity::Medium,
                });
            }
        }

        gaps
    }

    fn detect_action_gaps(&self, coverage_report: &CoverageReport) -> Vec<CoverageGap> {
        let mut gaps = Vec::new();

        for (action_name, coverage) in &coverage_report.action_coverage {
            if !coverage.is_covered {
                gaps.push(CoverageGap {
                    gap_type: CoverageGapType::UncoveredAction,
                    entity_type: None,
                    action_name: Some(action_name.clone()),
                    attribute_name: None,
                    description: format!("Action '{}' is not covered by any policy", action_name),
                    severity: GapSeverity::High,
                });
            }
        }

        gaps
    }
}

#[async_trait]
impl CoverageGapDetectionPort for CoverageGapDetectionAdapter {
    async fn detect_gaps(
        &self,
        coverage_report: &CoverageReport,
        _policies: &[Policy],
    ) -> Result<Vec<CoverageGap>, IamError> {
        let mut gaps = Vec::new();
        
        gaps.extend(self.detect_entity_gaps(coverage_report));
        gaps.extend(self.detect_action_gaps(coverage_report));

        Ok(gaps)
    }
}

pub struct CoverageSuggestionAdapter;

impl CoverageSuggestionAdapter {
    pub fn new() -> Self {
        Self
    }

    fn generate_entity_suggestions(&self, gaps: &[CoverageGap]) -> Vec<CoverageSuggestion> {
        gaps.iter()
            .filter(|gap| matches!(gap.gap_type, CoverageGapType::UncoveredEntity))
            .map(|gap| CoverageSuggestion {
                suggestion_type: SuggestionType::CreatePolicy,
                target_gap: gap.clone(),
                recommended_action: format!(
                    "Create a policy to cover entity type '{}'",
                    gap.entity_type.as_ref().unwrap()
                ),
                policy_template: Some(format!(
                    "permit (principal, action, resource) when {{ resource is {} }};",
                    gap.entity_type.as_ref().unwrap()
                )),
                priority: SuggestionPriority::High,
            })
            .collect()
    }

    fn generate_action_suggestions(&self, gaps: &[CoverageGap]) -> Vec<CoverageSuggestion> {
        gaps.iter()
            .filter(|gap| matches!(gap.gap_type, CoverageGapType::UncoveredAction))
            .map(|gap| CoverageSuggestion {
                suggestion_type: SuggestionType::CreatePolicy,
                target_gap: gap.clone(),
                recommended_action: format!(
                    "Create a policy to cover action '{}'",
                    gap.action_name.as_ref().unwrap()
                ),
                policy_template: Some(format!(
                    "permit (principal, action == \"{}\", resource);",
                    gap.action_name.as_ref().unwrap()
                )),
                priority: SuggestionPriority::High,
            })
            .collect()
    }
}

#[async_trait]
impl CoverageSuggestionPort for CoverageSuggestionAdapter {
    async fn generate_suggestions(
        &self,
        gaps: &[CoverageGap],
        _existing_policies: &[Policy],
    ) -> Result<Vec<CoverageSuggestion>, IamError> {
        let mut suggestions = Vec::new();
        
        suggestions.extend(self.generate_entity_suggestions(gaps));
        suggestions.extend(self.generate_action_suggestions(gaps));

        Ok(suggestions)
    }
}