use super::dto::{BatchPlaygroundRequest, BatchPlaygroundResponse};
use cedar_policy::{Entities, PolicySet};

use crate::features::policy_playground::dto as base;
use crate::shared::application::parallel::{
    build_entities as build_entities_shared,
    build_policy_set as build_policy_set_shared,
    evaluate_scenarios_channel,
    AuthScenario,
};
use tracing::info;

#[derive(Default)]
pub struct BatchEvalUseCase;

impl BatchEvalUseCase {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, req: &BatchPlaygroundRequest) -> Result<BatchPlaygroundResponse, String> {
        // Apply limit
        let scenarios = if let Some(limit) = req.limit_scenarios {
            req.scenarios.iter().cloned().take(limit).collect::<Vec<_>>()
        } else { req.scenarios.clone() };

        // Build shared PolicySet and Entities
        let pset = build_policy_set_shared(&req.policies).unwrap_or_else(|_| PolicySet::new());
        let entity_tuples: Vec<(String, std::collections::HashMap<String, serde_json::Value>, Vec<String>)> = req
            .entities
            .iter()
            .map(|e| (e.uid.clone(), e.attributes.clone(), e.parents.clone()))
            .collect();
        let ents = build_entities_shared(&entity_tuples).unwrap_or_else(|_| Entities::empty());

        // Build scenarios for the evaluator
        let total = scenarios.len();
        let auth_scenarios: Vec<AuthScenario> = scenarios
            .into_iter()
            .map(|s| AuthScenario {
                name: s.name,
                principal: s.principal,
                action: s.action,
                resource: s.resource,
                context: s.context,
            })
            .collect();

        // Use mpsc-based evaluator
        let workers = 8usize;
        let buffer = 2 * workers;
        let (outcomes, pstats) = evaluate_scenarios_channel(&pset, &ents, auth_scenarios, req.timeout_ms, workers, buffer).await?;

        let mut total_time = 0u64;
        let mut allow_count = 0usize;
        for o in outcomes.iter() {
            total_time += o.eval_time_us;
            if o.allow { allow_count += 1; }
        }

        let total = total;
        let statistics = base::EvaluationStatistics {
            total_scenarios: total,
            allow_count,
            deny_count: total.saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if total == 0 { 0 } else { total_time / total as u64 },
        };

        info!(
            scenarios_total = total,
            timeouts = pstats.timeouts,
            total_eval_time_us = pstats.total_eval_time_us,
            "batch_eval completed"
        );

        Ok(BatchPlaygroundResponse { results_count: total, statistics })
    }
}
