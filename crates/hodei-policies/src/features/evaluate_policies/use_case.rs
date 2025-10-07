use crate::features::evaluate_policies::dto::{
    Decision, EvaluatePoliciesCommand, EvaluationDecision,
};
use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use crate::internal::{schema_builder, translator};
use cedar_policy::{Authorizer, Context, Entities, EntityId, EntityTypeName, EntityUid, Request};
use std::str::FromStr;
use tracing::{info, warn};

pub struct EvaluatePoliciesUseCase;

impl Default for EvaluatePoliciesUseCase {
    fn default() -> Self {
        Self::new()
    }
}

impl EvaluatePoliciesUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        command: EvaluatePoliciesCommand<'_>,
    ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
        info!("Evaluating authorization request");

        // 1. Build Schema
        let schema = schema_builder::build_schema_from_entities(command.entities)?;
        info!("Schema built successfully");

        // 2. Translate Policies
        let cedar_policies = translator::to_cedar_policy_set(command.policies)?;
        info!("Policies translated successfully");

        // 3. Translate Entities
        let mut cedar_entities = Vec::new();
        for entity in command.entities {
            let cedar_entity = translator::to_cedar_entity(*entity)?;
            cedar_entities.push(cedar_entity);
        }
        let entities =
            Entities::from_entities(cedar_entities.into_iter(), Some(&schema)).map_err(|e| {
                EvaluatePoliciesError::TranslationError(format!("Entities creation failed: {}", e))
            })?;
        info!("Entities translated successfully");

        // 4. Build Cedar Request
        let principal_euid = translator::to_cedar_euid(command.request.principal_hrn)?;
        let resource_euid = translator::to_cedar_euid(command.request.resource_hrn)?;
        let action_type_name = EntityTypeName::from_str("Action").map_err(|e| {
            EvaluatePoliciesError::TranslationError(format!("Invalid action type name: {}", e))
        })?;
        let action_entity_id = EntityId::new(command.request.action);
        let action_euid = EntityUid::from_type_name_and_id(action_type_name, action_entity_id);

        let context = if let Some(ctx) = &command.request.context {
            let map: serde_json::Map<String, serde_json::Value> = ctx.clone().into_iter().collect();
            let json_value = serde_json::Value::Object(map);
            Context::from_json_value(json_value, Some((&schema, &principal_euid))).map_err(|e| {
                EvaluatePoliciesError::TranslationError(format!("Context creation failed: {}", e))
            })?
        } else {
            Context::empty()
        };

        let request = Request::new(
            principal_euid,
            action_euid,
            resource_euid,
            context,
            Some(&schema),
        )
        .map_err(|e| {
            EvaluatePoliciesError::TranslationError(format!("Request creation failed: {}", e))
        })?;
        info!("Cedar request built successfully");

        // 5. Evaluate
        let authorizer = Authorizer::new();
        let response = authorizer.is_authorized(&request, &cedar_policies, &entities);
        info!("Authorization evaluation completed");

        // 6. Map Response
        let decision = match response.decision() {
            cedar_policy::Decision::Allow => Decision::Allow,
            cedar_policy::Decision::Deny => Decision::Deny,
        };

        // Simplified: no determining policies for now
        let determining_policies = vec![];

        // Simplified: no reasons for now
        let reasons = vec![];

        warn!("Authorization decision: {:?}", decision);

        Ok(EvaluationDecision {
            decision,
            determining_policies,
            reasons,
        })
    }
}
