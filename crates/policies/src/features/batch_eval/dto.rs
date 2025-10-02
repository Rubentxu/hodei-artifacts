use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto::{AuthorizationScenario, EntityDefinition, EvaluationStatistics};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchPlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub scenarios: Vec<AuthorizationScenario>,
    #[serde(default)]
    pub limit_scenarios: Option<usize>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPlaygroundResponse {
    pub results_count: usize,
    pub statistics: EvaluationStatistics,
}
