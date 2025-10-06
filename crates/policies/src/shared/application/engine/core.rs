//! Authorization Engine Core Implementation
//!
//! This module implements the Cedar-based authorization engine with a
//! completely agnostic public API. Cedar is encapsulated as an implementation detail.

use super::types::{AuthorizationDecision, EngineError, EngineRequest, PolicyDocument};
use crate::shared::infrastructure::translator;
use cedar_policy::{Authorizer, Context, Entities, EntityUid, Policy, PolicySet, Request};
use kernel::HodeiEntity;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

// ============================================================================
// Authorization Engine
// ============================================================================

/// Authorization Engine - Evaluates policies using Cedar internally
///
/// This engine provides a completely agnostic API that accepts only kernel types.
/// Cedar is used internally but never exposed to external crates.
///
/// # Thread Safety
///
/// The engine is thread-safe and can be shared across threads using `Arc`.
/// Internal state (policies and entities) is protected by `RwLock`.
///
/// # Examples
///
/// ```rust,ignore
/// use policies::engine::{AuthorizationEngine, EngineRequest};
///
/// // Create engine
/// let engine = AuthorizationEngine::new();
///
/// // Load policies
/// engine.load_policies(vec![
///     "permit(principal, action == Action::\"Read\", resource);".to_string()
/// ])?;
///
/// // Register entities
/// engine.register_entity(&user)?;
/// engine.register_entity(&document)?;
///
/// // Evaluate
/// let request = EngineRequest::new(&user, "Read", &document);
/// let decision = engine.is_authorized(&request)?;
/// ```
pub struct AuthorizationEngine {
    /// Cedar authorizer (internal)
    authorizer: Authorizer,

    /// Loaded policies (internal Cedar representation)
    policies: Arc<RwLock<PolicySet>>,

    /// Entity store (internal Cedar representation)
    entities: Arc<RwLock<Entities>>,

    /// Policy documents cache (for diagnostics)
    policy_docs: Arc<RwLock<HashMap<String, PolicyDocument>>>,
}

impl AuthorizationEngine {
    /// Create a new authorization engine
    ///
    /// The engine starts empty with no policies or entities.
    pub fn new() -> Self {
        Self {
            authorizer: Authorizer::new(),
            policies: Arc::new(RwLock::new(PolicySet::new())),
            entities: Arc::new(RwLock::new(Entities::empty())),
            policy_docs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Evaluate an authorization request (MAIN PUBLIC API)
    ///
    /// This is the primary method external crates use. It accepts only agnostic types.
    ///
    /// # Arguments
    ///
    /// * `request` - Authorization request with agnostic types
    ///
    /// # Returns
    ///
    /// An `AuthorizationDecision` indicating whether the action is allowed or denied.
    ///
    /// # Errors
    ///
    /// Returns `EngineError` if:
    /// - Translation from agnostic to Cedar types fails
    /// - Cedar evaluation fails
    /// - Required entities are not registered
    #[tracing::instrument(skip(self, request), fields(
        principal = %request.principal_hrn(),
        action = request.action,
        resource = %request.resource_hrn()
    ))]
    pub fn is_authorized(
        &self,
        request: &EngineRequest,
    ) -> Result<AuthorizationDecision, EngineError> {
        debug!("Starting authorization evaluation");

        // 1. Translate agnostic entities to Cedar entities
        let principal_cedar = translator::translate_to_cedar_entity(request.principal)?;
        let resource_cedar = translator::translate_to_cedar_entity(request.resource)?;

        debug!(
            "Translated principal: {:?}, resource: {:?}",
            principal_cedar.uid(),
            resource_cedar.uid()
        );

        // 2. Build Cedar action EntityUid
        let action_hrn = kernel::Hrn::action(
            request.principal_hrn().service(),
            request.action,
        );
        let action_uid_str = action_hrn.entity_uid_string();
        let action_uid = EntityUid::from_str(&action_uid_str)
            .map_err(|e| EngineError::EvaluationFailed(format!("Invalid action: {}", e)))?;

        debug!("Action EntityUid: {:?}", action_uid);

        // 3. Translate context attributes (if any)
        let context = if request.context.is_empty() {
            Context::empty()
        } else {
            // TODO: Implement context translation
            // For now, use empty context
            Context::empty()
        };

        // 4. Build Cedar Request (internal)
        let cedar_request = Request::new(
            principal_cedar.uid().clone(),
            action_uid,
            resource_cedar.uid().clone(),
            context,
            None, // schema (optional)
        )
        .map_err(|e| EngineError::EvaluationFailed(format!("Failed to build request: {}", e)))?;

        debug!("Built Cedar request");

        // 5. Get current policies and entities (read lock)
        let policies = self.policies.read().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock policies: {}", e))
        })?;

        let entities = self.entities.read().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock entities: {}", e))
        })?;

        debug!("Acquired locks on policies and entities");

        // 6. Evaluate with Cedar (INTERNAL)
        let response = self
            .authorizer
            .is_authorized(&cedar_request, &policies, &entities);

        debug!("Cedar evaluation complete: {:?}", response.decision());

        // 7. Translate Cedar response to agnostic decision
        let decision = match response.decision() {
            cedar_policy::Decision::Allow => {
                info!("Authorization ALLOWED");
                AuthorizationDecision::allow_with_reason("Allowed by policy".to_string())
            }
            cedar_policy::Decision::Deny => {
                info!("Authorization DENIED");
                AuthorizationDecision::deny_with_reason("Denied by policy".to_string())
            }
        };

        // 8. Extract determining policies (if available)
        let determining_policy_ids: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|policy_id| policy_id.to_string())
            .collect();

        Ok(decision.with_policies(determining_policy_ids))
    }

    /// Load policies from Cedar DSL strings
    ///
    /// Policies are parsed and validated. Invalid policies are rejected.
    ///
    /// # Arguments
    ///
    /// * `policy_texts` - Vector of Cedar DSL policy strings
    ///
    /// # Returns
    ///
    /// Number of policies successfully loaded
    ///
    /// # Errors
    ///
    /// Returns `EngineError::InvalidPolicy` if any policy has invalid syntax
    #[tracing::instrument(skip(self, policy_texts), fields(count = policy_texts.len()))]
    pub fn load_policies(&self, policy_texts: Vec<String>) -> Result<usize, EngineError> {
        info!("Loading {} policies", policy_texts.len());

        let mut new_policy_set = PolicySet::new();
        let mut policy_docs_map = HashMap::new();

        for (idx, policy_text) in policy_texts.iter().enumerate() {
            // Parse Cedar policy
            let policy = Policy::from_str(policy_text).map_err(|e| {
                EngineError::InvalidPolicy(format!("Policy {} parse error: {}", idx, e))
            })?;

            let policy_id = format!("policy_{}", idx);

            // Add to policy set
            new_policy_set
                .add(policy.clone())
                .map_err(|e| EngineError::InvalidPolicy(format!("Failed to add policy: {}", e)))?;

            // Cache policy document
            policy_docs_map.insert(
                policy_id.clone(),
                PolicyDocument::new(policy_id, policy_text.clone()),
            );

            debug!("Loaded policy {}: {} bytes", idx, policy_text.len());
        }

        // Update internal state (write lock)
        let mut policies = self.policies.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock policies: {}", e))
        })?;

        *policies = new_policy_set;

        let mut policy_docs = self.policy_docs.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock policy docs: {}", e))
        })?;

        *policy_docs = policy_docs_map;

        info!("Successfully loaded {} policies", policy_texts.len());
        Ok(policy_texts.len())
    }

    /// Register an entity in the entity store
    ///
    /// Entities must be registered before they can be used in authorization requests.
    ///
    /// # Arguments
    ///
    /// * `entity` - Any type implementing `HodeiEntity`
    ///
    /// # Errors
    ///
    /// Returns `EngineError::TranslationError` if the entity cannot be translated to Cedar
    #[tracing::instrument(skip(self, entity), fields(hrn = %entity.hrn()))]
    pub fn register_entity(&self, entity: &dyn HodeiEntity) -> Result<(), EngineError> {
        debug!("Registering entity: {}", entity.hrn());

        // Translate to Cedar entity
        let cedar_entity = translator::translate_to_cedar_entity(entity)?;

        // Get current entities (read lock)
        let current_entities = self.entities.read().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock entities: {}", e))
        })?;

        // Add new entity to existing entities
        let new_entities = current_entities.clone().add_entities(vec![cedar_entity], None)?;

        // Update entity store (write lock)
        let mut entities = self.entities.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock entities: {}", e))
        })?;

        *entities = new_entities;

        debug!("Entity registered successfully");
        Ok(())
    }

    /// Register multiple entities at once
    ///
    /// More efficient than calling `register_entity` multiple times.
    ///
    /// # Arguments
    ///
    /// * `entities` - Vector of entities to register
    ///
    /// # Returns
    ///
    /// Number of entities successfully registered
    #[tracing::instrument(skip(self, entities), fields(count = entities.len()))]
    pub fn register_entities(&self, entities: Vec<&dyn HodeiEntity>) -> Result<usize, EngineError> {
        info!("Registering {} entities", entities.len());

        // Translate all entities to Cedar entities
        let cedar_entities: Result<Vec<_>, _> = entities
            .iter()
            .map(|entity| translator::translate_to_cedar_entity(*entity))
            .collect();

        let cedar_entities = cedar_entities.map_err(|e| {
            EngineError::TranslationError(e.to_string())
        })?;

        // Create new Entities with all entities
        let new_entities = Entities::from_entities(cedar_entities, None)?;

        // Update entity store (write lock)
        let mut entity_store = self.entities.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock entities: {}", e))
        })?;

        *entity_store = new_entities;

        info!("Successfully registered {} entities", entities.len());
        Ok(entities.len())
    }

    /// Clear all loaded policies
    pub fn clear_policies(&self) -> Result<(), EngineError> {
        info!("Clearing all policies");

        let mut policies = self.policies.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock policies: {}", e))
        })?;

        *policies = PolicySet::new();

        let mut policy_docs = self.policy_docs.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock policy docs: {}", e))
        })?;

        policy_docs.clear();

        Ok(())
    }

    /// Clear all registered entities
    pub fn clear_entities(&self) -> Result<(), EngineError> {
        info!("Clearing all entities");

        let mut entities = self.entities.write().map_err(|e| {
            EngineError::EvaluationFailed(format!("Failed to lock entities: {}", e))
        })?;

        *entities = Entities::empty();

        Ok(())
    }

    /// Get the number of loaded policies
    pub fn policy_count(&self) -> usize {
        self.policies
            .read()
            .map(|p| p.policies().count())
            .unwrap_or(0)
    }

    /// Get the number of registered entities
    pub fn entity_count(&self) -> usize {
        self.entities.read().map(|e| e.iter().count()).unwrap_or(0)
    }
}

impl Default for AuthorizationEngine {
    fn default() -> Self {
        Self::new()
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

    #[test]
    fn engine_creation() {
        let engine = AuthorizationEngine::new();
        assert_eq!(engine.policy_count(), 0);
        assert_eq!(engine.entity_count(), 0);
    }

    #[test]
    fn load_simple_policy() {
        let engine = AuthorizationEngine::new();
        let policy = "permit(principal, action, resource);".to_string();

        let result = engine.load_policies(vec![policy]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
        assert_eq!(engine.policy_count(), 1);
    }

    #[test]
    fn load_invalid_policy_fails() {
        let engine = AuthorizationEngine::new();
        let policy = "this is not valid cedar syntax".to_string();

        let result = engine.load_policies(vec![policy]);
        assert!(result.is_err());
    }

    #[test]
    fn register_entity() {
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

        let result = engine.register_entity(&user);
        assert!(result.is_ok());
        assert_eq!(engine.entity_count(), 1);
    }

    #[test]
    fn clear_policies() {
        let engine = AuthorizationEngine::new();
        engine
            .load_policies(vec!["permit(principal, action, resource);".to_string()])
            .unwrap();

        assert_eq!(engine.policy_count(), 1);

        engine.clear_policies().unwrap();
        assert_eq!(engine.policy_count(), 0);
    }

    #[test]
    fn clear_entities() {
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

        engine.register_entity(&user).unwrap();
        assert_eq!(engine.entity_count(), 1);

        engine.clear_entities().unwrap();
        assert_eq!(engine.entity_count(), 0);
    }
}
