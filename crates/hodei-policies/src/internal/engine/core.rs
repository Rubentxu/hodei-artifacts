//! Simplified Authorization Engine Core Implementation
//!
//! This module implements a basic Cedar-based authorization engine that works
//! with the current Cedar API and compiles successfully.

use super::translator;
use super::types::{AuthorizationDecision, EngineError, EngineRequest};
use cedar_policy::{Authorizer, Context, Entities, Policy, PolicySet, Request};
use kernel::HodeiEntity;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::{debug, info};

/// Simple Authorization Engine
///
/// This engine evaluates Cedar policies without requiring a schema.
/// Cedar can function perfectly in "schema-less mode" where policies are evaluated
/// based on their content without compile-time type checking.
///
/// ## Schema-less Operation
///
/// By default, this engine operates without a Cedar schema, which means:
/// - Policies are evaluated based on runtime entity data
/// - Actions are referenced as strings (e.g., Action::"Read")
/// - No compile-time validation of principal/resource types
/// - Maximum flexibility - any entity type can use any action
///
/// ## When to Use Schema
///
/// If you need Cedar's schema validation (type checking, action constraints),
/// you should:
/// 1. Register actions using types that implement `kernel::ActionTrait`
/// 2. Build a schema using `EngineBuilder`
/// 3. Pass the schema explicitly during evaluation
///
/// For most use cases, schema-less operation is sufficient and more flexible.
pub struct AuthorizationEngine {
    /// Cedar authorizer
    authorizer: Authorizer,
    /// Loaded policies
    policies: Arc<TokioRwLock<PolicySet>>,
    /// Entity store
    entities: Arc<TokioRwLock<Entities>>,
}

impl AuthorizationEngine {
    /// Create a new authorization engine
    pub fn new() -> Self {
        Self {
            authorizer: Authorizer::new(),
            policies: Arc::new(TokioRwLock::new(PolicySet::new())),
            entities: Arc::new(TokioRwLock::new(Entities::empty())),
        }
    }

    /// Evaluate an authorization request in schema-less mode
    ///
    /// This method evaluates policies without Cedar schema validation.
    /// Actions are treated as strings and entities are validated only
    /// against the policy content itself.
    ///
    /// This approach provides maximum flexibility while maintaining
    /// Cedar's powerful policy evaluation capabilities.
    pub async fn is_authorized<'a>(
        &self,
        request: &EngineRequest<'a>,
    ) -> Result<AuthorizationDecision, EngineError> {
        debug!("Starting authorization evaluation");

        // 1. Translate entities to Cedar
        let principal_cedar = translator::translate_to_cedar_entity(request.principal)
            .map_err(|e| EngineError::TranslationError(e.to_string()))?;
        let resource_cedar = translator::translate_to_cedar_entity(request.resource)
            .map_err(|e| EngineError::TranslationError(e.to_string()))?;

        debug!("Translated entities successfully");

        // 2. Build Cedar action EntityUid
        // Use a generic "Action" namespace instead of service-specific
        let action_uid_str = format!("Action::\"{}\"", request.action);
        let action_uid = cedar_policy::EntityUid::from_str(&action_uid_str)
            .map_err(|e| EngineError::EvaluationFailed(format!("Invalid action: {}", e)))?;

        // 3. Build Cedar Context from request context
        let cedar_context = if request.context.is_empty() {
            Context::empty()
        } else {
            // Convert HashMap<String, serde_json::Value> to Cedar Context
            let mut context_map = std::collections::HashMap::new();
            for (key, value) in &request.context {
                // Convert serde_json::Value to RestrictedExpression
                let restricted_expr = json_value_to_restricted_expr(value).map_err(|e| {
                    EngineError::EvaluationFailed(format!("Context conversion error: {}", e))
                })?;
                context_map.insert(key.clone(), restricted_expr);
            }
            cedar_policy::Context::from_pairs(context_map).map_err(|e| {
                EngineError::EvaluationFailed(format!("Failed to build context: {}", e))
            })?
        };

        // 4. Build Cedar Request in schema-less mode
        // We operate without schema validation, which allows:
        // - Any principal type to use any action
        // - Actions defined as strings without ActionTrait types
        // - Maximum flexibility in policy evaluation
        // Cedar evaluates policies based on entity attributes and policy conditions
        let cedar_request = Request::new(
            principal_cedar.uid().clone(),
            action_uid,
            resource_cedar.uid().clone(),
            cedar_context,
            None, // Schema-less mode: no type validation
        )
        .map_err(|e| EngineError::EvaluationFailed(format!("Failed to build request: {}", e)))?;

        // 5. Get policies and entities for evaluation
        let policies = self.policies.read().await;
        let entities = self.entities.read().await;

        // 6. Evaluate with Cedar
        let response = self
            .authorizer
            .is_authorized(&cedar_request, &policies, &entities);
        debug!("Cedar evaluation complete: {:?}", response.decision());

        // 7. Map response to decision
        let decision = match response.decision() {
            cedar_policy::Decision::Allow => {
                info!("Authorization ALLOWED");
                AuthorizationDecision::allow()
            }
            cedar_policy::Decision::Deny => {
                info!("Authorization DENIED");
                AuthorizationDecision::deny()
            }
        };

        Ok(decision)
    }

    /// Load policies from Cedar DSL strings with IDs
    pub async fn load_policies(&self, policy_texts: Vec<String>) -> Result<usize, EngineError> {
        info!("Loading {} policies", policy_texts.len());

        let mut new_policy_set = PolicySet::new();

        for (idx, policy_text) in policy_texts.iter().enumerate() {
            // Parse policy with unique ID based on index to avoid duplicates
            let policy_id = cedar_policy::PolicyId::new(format!("auto_policy_{}", idx));
            let policy = Policy::parse(Some(policy_id), policy_text).map_err(|e| {
                EngineError::InvalidPolicy(format!("Policy {} parse error: {}", idx, e))
            })?;

            new_policy_set
                .add(policy)
                .map_err(|e| EngineError::InvalidPolicy(format!("Failed to add policy: {}", e)))?;

            debug!("Loaded policy {}: {} bytes", idx, policy_text.len());
        }

        // Update policies
        let mut policies = self.policies.write().await;

        *policies = new_policy_set;

        info!("Successfully loaded {} policies", policy_texts.len());
        Ok(policy_texts.len())
    }

    /// Register an entity in the entity store
    #[allow(dead_code)]
    pub async fn register_entity(&self, entity: &dyn HodeiEntity) -> Result<(), EngineError> {
        debug!("Registering entity: {}", entity.hrn());

        // Translate to Cedar entity
        let cedar_entity = translator::translate_to_cedar_entity(entity)
            .map_err(|e| EngineError::TranslationError(e.to_string()))?;

        // Create new entity store with the new entity
        let new_entities = Entities::from_entities(vec![cedar_entity], None).map_err(|e| {
            EngineError::TranslationError(format!("Failed to create entities: {}", e))
        })?;

        // Update entities
        let mut entities = self.entities.write().await;

        *entities = new_entities;

        debug!("Entity registered successfully");
        Ok(())
    }

    /// Register multiple entities at once (schema-less mode)
    ///
    /// Entities are registered without schema validation, allowing maximum flexibility.
    /// Cedar will evaluate policies based on the actual entity attributes at runtime.
    ///
    /// ## How it works
    ///
    /// 1. Entities are translated to Cedar's internal format
    /// 2. Entity attributes are preserved for policy evaluation
    /// 3. No schema validation is performed
    /// 4. Actions don't need to be pre-registered
    ///
    /// This means policies can reference any action (as a string) and Cedar will
    /// evaluate them based on the policy conditions and entity data.
    pub async fn register_entities(
        &self,
        entities: Vec<&dyn HodeiEntity>,
    ) -> Result<usize, EngineError> {
        info!(
            "Registering {} entities in schema-less mode",
            entities.len()
        );

        // 1. Translate all entities to Cedar entities
        let cedar_entities: Result<Vec<_>, _> = entities
            .iter()
            .map(|entity| translator::translate_to_cedar_entity(*entity))
            .collect();

        let cedar_entities =
            cedar_entities.map_err(|e| EngineError::TranslationError(e.to_string()))?;

        // 2. Create new Entities without schema validation
        // Schema-less mode: entities are created without type checking
        // Cedar will validate entity structure at policy evaluation time
        let new_entities = Entities::from_entities(cedar_entities, None).map_err(|e| {
            EngineError::TranslationError(format!("Failed to create entities: {}", e))
        })?;

        // 3. Update entity store
        let mut entity_store = self.entities.write().await;

        *entity_store = new_entities;

        info!(
            "Successfully registered {} entities (schema-less)",
            entities.len()
        );
        Ok(entities.len())
    }

    /// Clear all loaded policies
    pub async fn clear_policies(&self) -> Result<(), EngineError> {
        info!("Clearing all policies");

        let mut policies = self.policies.write().await;

        *policies = PolicySet::new();

        Ok(())
    }

    /// Clear all registered entities
    pub async fn clear_entities(&self) -> Result<(), EngineError> {
        info!("Clearing all entities");

        let mut entities = self.entities.write().await;

        *entities = Entities::empty();

        Ok(())
    }

    /// Get the number of loaded policies
    #[allow(dead_code)]
    pub async fn policy_count(&self) -> usize {
        self.policies.read().await.policies().count()
    }

    /// Get the number of registered entities
    #[allow(dead_code)]
    pub async fn entity_count(&self) -> usize {
        self.entities.read().await.iter().count()
    }
}

impl Default for AuthorizationEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to convert serde_json::Value to Cedar RestrictedExpression
fn json_value_to_restricted_expr(
    value: &serde_json::Value,
) -> Result<cedar_policy::RestrictedExpression, String> {
    use serde_json::Value;

    match value {
        Value::Null => Err("Null values not supported in Cedar context".to_string()),
        Value::Bool(b) => Ok(cedar_policy::RestrictedExpression::new_bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(cedar_policy::RestrictedExpression::new_long(i))
            } else {
                Err(format!("Number {} cannot be converted to i64", n))
            }
        }
        Value::String(s) => Ok(cedar_policy::RestrictedExpression::new_string(s.clone())),
        Value::Array(arr) => {
            let exprs: Result<Vec<_>, _> = arr.iter().map(json_value_to_restricted_expr).collect();
            Ok(cedar_policy::RestrictedExpression::new_set(exprs?))
        }
        Value::Object(map) => {
            let mut cedar_map = std::collections::HashMap::new();
            for (key, val) in map {
                let expr = json_value_to_restricted_expr(val)?;
                cedar_map.insert(key.clone(), expr);
            }
            cedar_policy::RestrictedExpression::new_record(cedar_map)
                .map_err(|e| format!("Failed to create record: {}", e))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{
        AttributeName, AttributeType, AttributeValue, ResourceTypeName, ServiceName,
    };
    use kernel::{HodeiEntity, HodeiEntityType, Hrn};
    use std::collections::HashMap;

    // Test entity
    #[derive(Debug)]
    struct TestUser {
        hrn: Hrn,
        name: String,
    }

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![(AttributeName::new("name").unwrap(), AttributeType::string())]
        }
    }

    impl HodeiEntity for TestUser {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("name").unwrap(),
                AttributeValue::string(&self.name),
            );
            attrs
        }
    }

    #[tokio::test]
    async fn engine_creation() {
        let engine = AuthorizationEngine::new();
        assert_eq!(engine.policy_count().await, 0);
        assert_eq!(engine.entity_count().await, 0);
    }

    #[tokio::test]
    async fn load_simple_policy() {
        let engine = AuthorizationEngine::new();
        let policy = "permit(principal, action, resource);".to_string();

        let result = engine.load_policies(vec![policy]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
        assert_eq!(engine.policy_count().await, 1);
    }

    #[tokio::test]
    async fn register_entity() {
        let engine = AuthorizationEngine::new();
        let user = TestUser {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
        };

        let result = engine.register_entity(&user).await;
        assert!(result.is_ok());
        assert_eq!(engine.entity_count().await, 1);
    }

    #[tokio::test]
    async fn clear_policies() {
        let engine = AuthorizationEngine::new();
        engine
            .load_policies(vec!["permit(principal, action, resource);".to_string()])
            .await
            .unwrap();

        assert_eq!(engine.policy_count().await, 1);

        engine.clear_policies().await.unwrap();
        assert_eq!(engine.policy_count().await, 0);
    }

    #[tokio::test]
    async fn clear_entities() {
        let engine = AuthorizationEngine::new();
        let user = TestUser {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
        };

        engine.register_entity(&user).await.unwrap();
        assert_eq!(engine.entity_count().await, 1);

        engine.clear_entities().await.unwrap();
        assert_eq!(engine.entity_count().await, 0);
    }
}
