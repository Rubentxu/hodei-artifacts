//! Policy Evaluator Adapter for Playground Evaluate
//!
//! This adapter implements the PolicyEvaluatorPort trait by using Cedar's
//! authorization engine to evaluate policies against requests.

use super::super::dto::{
    Decision, DeterminingPolicy, PlaygroundAuthorizationRequest, PlaygroundEvaluationOutcome,
    PolicyEffect,
};
use super::super::error::PlaygroundEvaluateError;
use super::super::ports::PolicyEvaluatorPort;
use async_trait::async_trait;
use cedar_policy::{
    Authorizer, Context, Entities, EntityUid, Policy, PolicySet, Request, RestrictedExpression,
    Schema,
};
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{debug, info, warn};

use crate::internal::engine::authority::{self, ActionResolution};

/// Adapter that implements PolicyEvaluatorPort using Cedar's Authorizer
///
/// This adapter creates a Cedar authorization engine, loads inline policies,
/// and evaluates authorization requests to produce Allow/Deny decisions.
///
/// # Architecture
///
/// This adapter bridges the playground_evaluate feature with Cedar's native
/// authorization capabilities, handling:
/// - Policy parsing and loading
/// - Request construction
/// - Authorization evaluation
/// - Result translation
pub struct PolicyEvaluatorAdapter;

impl PolicyEvaluatorAdapter {
    /// Create a new policy evaluator adapter
    pub fn new() -> Self {
        Self
    }

    /// Parse policy texts into a Cedar PolicySet
    ///
    /// # Arguments
    ///
    /// * `policy_texts` - List of Cedar policy strings
    ///
    /// # Returns
    ///
    /// A Cedar PolicySet containing all parsed policies
    ///
    /// # Errors
    ///
    /// Returns an error if any policy fails to parse
    fn parse_policies(
        &self,
        policy_texts: &[String],
    ) -> Result<PolicySet, PlaygroundEvaluateError> {
        debug!(policy_count = policy_texts.len(), "Parsing policies");

        let mut policy_set = PolicySet::new();

        for (index, policy_text) in policy_texts.iter().enumerate() {
            let policy = Policy::from_str(policy_text).map_err(|e| {
                warn!(policy_index = index, error = %e, "Policy parsing failed");
                PlaygroundEvaluateError::PolicyError(format!("Policy {} parse error: {}", index, e))
            })?;

            policy_set.add(policy).map_err(|e| {
                warn!(policy_index = index, error = %e, "Failed to add policy to set");
                PlaygroundEvaluateError::PolicyError(format!(
                    "Failed to add policy {}: {}",
                    index, e
                ))
            })?;
        }

        info!(
            policy_count = policy_set.policies().count(),
            "Policies parsed successfully"
        );
        Ok(policy_set)
    }

    /// Convert an HRN to a Cedar EntityUid
    ///
    /// The UID is built with the typed Cedar APIs, never by interpolating
    /// the HRN text: the identifier carries the whole scope (partition,
    /// service, region, account, type and id), so two homonyms of different
    /// accounts are different entities.
    ///
    /// # Arguments
    ///
    /// * `hrn` - The HRN to convert
    ///
    /// # Returns
    ///
    /// A Cedar EntityUid
    ///
    /// # Errors
    ///
    /// Returns an error if the HRN cannot be converted
    fn hrn_to_entity_uid(&self, hrn: &kernel::Hrn) -> Result<EntityUid, PlaygroundEvaluateError> {
        crate::internal::engine::translator::translate_to_cedar_euid(hrn).map_err(|e| {
            warn!(hrn = %hrn, error = %e, "Failed to convert HRN to EntityUid");
            PlaygroundEvaluateError::InvalidRequest(format!("Invalid HRN '{}': {}", hrn, e))
        })
    }

    /// Convert an action HRN to a Cedar EntityUid
    ///
    /// An action HRN denotes a verb, not a resource: it maps to the flat
    /// `Action::"name"` form, the same form the evaluation engine and the
    /// registered schema use.
    fn action_to_entity_uid(
        &self,
        hrn: &kernel::Hrn,
    ) -> Result<EntityUid, PlaygroundEvaluateError> {
        crate::internal::engine::translator::translate_hrn_to_cedar_euid(hrn).map_err(|e| {
            warn!(hrn = %hrn, error = %e, "Failed to convert action HRN to EntityUid");
            PlaygroundEvaluateError::InvalidRequest(format!("Invalid action '{}': {}", hrn, e))
        })
    }

    /// Build a Cedar authorization request
    ///
    /// The action is the one the **schema** resolved, and the schema travels
    /// with the request so that Cedar itself checks that the request
    /// conforms to it. A request that does not conform is not built: it is
    /// denied, with the reason reported.
    ///
    /// # Arguments
    ///
    /// * `request` - The playground authorization request
    /// * `action` - The action UID the schema resolved
    /// * `context` - The request context, already converted
    /// * `schema` - The schema to check the request against, if it constrains
    ///
    /// # Returns
    ///
    /// A Cedar Request, or the reason why the request does not conform
    fn build_cedar_request(
        &self,
        request: &PlaygroundAuthorizationRequest,
        action: EntityUid,
        context: &HashMap<String, RestrictedExpression>,
        schema: Option<&Schema>,
    ) -> Result<Request, RequestBuildError> {
        debug!("Building Cedar authorization request");

        let principal = self
            .hrn_to_entity_uid(&request.principal)
            .map_err(RequestBuildError::Invalid)?;
        let resource = self
            .hrn_to_entity_uid(&request.resource)
            .map_err(RequestBuildError::Invalid)?;

        let cedar_context = Context::from_pairs(context.clone()).map_err(|e| {
            warn!(error = %e, "Failed to build the Cedar context");
            RequestBuildError::Invalid(PlaygroundEvaluateError::InvalidRequest(format!(
                "the context could not be built: {}",
                e
            )))
        })?;

        Request::new(principal, action, resource, cedar_context, schema).map_err(|e| {
            warn!(error = %e, "The request does not conform to the schema");
            RequestBuildError::NonConformant(format!(
                "the request does not conform to the schema: {}",
                e
            ))
        })
    }

    /// Translate Cedar authorization response to playground decision
    ///
    /// # Arguments
    ///
    /// * `response` - Cedar authorization response
    ///
    /// # Returns
    ///
    /// A tuple of (Decision, Vec<DeterminingPolicy>, Vec<String>) where the
    /// last one holds the errors the authorization itself reported
    fn translate_response(
        &self,
        response: &cedar_policy::Response,
    ) -> (Decision, Vec<DeterminingPolicy>, Vec<String>) {
        let decision = match response.decision() {
            cedar_policy::Decision::Allow => Decision::Allow,
            cedar_policy::Decision::Deny => Decision::Deny,
        };

        let mut determining_policies = Vec::new();

        // Extract determining policies from response
        for policy_id in response.diagnostics().reason() {
            let policy_id_str = policy_id.to_string();

            let effect = if decision == Decision::Allow {
                PolicyEffect::Permit
            } else {
                PolicyEffect::Forbid
            };

            let determining_policy = DeterminingPolicy::new(policy_id_str, effect);
            determining_policies.push(determining_policy);
        }

        // The errors the authorization reported belong to the consumer: a
        // denial caused by an evaluation error is not a denial caused by the
        // policies.
        let errors: Vec<String> = response
            .diagnostics()
            .errors()
            .map(|e| e.to_string())
            .collect();

        debug!(
            decision = ?decision,
            determining_count = determining_policies.len(),
            error_count = errors.len(),
            "Translated Cedar response"
        );

        (decision, determining_policies, errors)
    }
}

impl Default for PolicyEvaluatorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyEvaluatorPort for PolicyEvaluatorAdapter {
    async fn evaluate(
        &self,
        request: &PlaygroundAuthorizationRequest,
        policy_texts: &[String],
        schema: &Schema,
        context: &HashMap<String, RestrictedExpression>,
    ) -> Result<PlaygroundEvaluationOutcome, PlaygroundEvaluateError> {
        info!(
            principal = %request.principal,
            action = %request.action,
            resource = %request.resource,
            policy_count = policy_texts.len(),
            "Evaluating authorization request"
        );

        // Parse policies
        let policy_set = self.parse_policies(policy_texts)?;

        // The schema is the authority over the request. A schema that
        // declares nothing constrains nothing, so it is not applied; a schema
        // that declares something decides which action the requested verb
        // denotes.
        let action_id = request.action.resource_id();
        let (action, schema_for_request) = if authority::declares_anything(schema) {
            match authority::resolve_action(schema, action_id) {
                ActionResolution::Resolved(uid) => (uid, Some(schema)),
                ActionResolution::NotDeclared => {
                    warn!(action = %action_id, "The action is not declared in the schema");
                    return Ok(PlaygroundEvaluationOutcome::denied(vec![format!(
                        "the action `{}` is not declared in the schema",
                        action_id
                    )]));
                }
                ActionResolution::Ambiguous(candidates) => {
                    let names: Vec<String> = candidates.iter().map(|uid| uid.to_string()).collect();
                    warn!(action = %action_id, "The action is ambiguous in the schema");
                    return Ok(PlaygroundEvaluationOutcome::denied(vec![format!(
                        "the action `{}` is declared more than once in the schema: {}",
                        action_id,
                        names.join(", ")
                    )]));
                }
            }
        } else {
            // The schema says nothing about the world, so it cannot reject
            // anything: the request is evaluated without it, exactly as the
            // validation path checks the syntax alone.
            (self.action_to_entity_uid(&request.action)?, None)
        };

        // Build Cedar request
        let cedar_request =
            match self.build_cedar_request(request, action, context, schema_for_request) {
                Ok(cedar_request) => cedar_request,
                Err(RequestBuildError::NonConformant(reason)) => {
                    // The request does not conform to the schema it was given:
                    // fail closed, and say why.
                    warn!(reason = %reason, "The request does not conform to the schema");
                    return Ok(PlaygroundEvaluationOutcome::denied(vec![reason]));
                }
                Err(RequestBuildError::Invalid(error)) => return Err(error),
            };

        // Create empty entities (no entity data for now)
        let entities = Entities::empty();

        // Create authorizer
        let authorizer = Authorizer::new();

        // Evaluate
        let response = authorizer.is_authorized(&cedar_request, &policy_set, &entities);

        // Translate response
        let (decision, determining_policies, errors) = self.translate_response(&response);

        info!(
            decision = ?decision,
            determining_policies = determining_policies.len(),
            errors = errors.len(),
            "Authorization evaluation complete"
        );

        Ok(PlaygroundEvaluationOutcome::new(
            decision,
            determining_policies,
            errors,
        ))
    }
}

/// Why the Cedar request could not be built.
#[derive(Debug)]
enum RequestBuildError {
    /// The request is malformed: an HRN that is not one, a context that
    /// cannot be expressed.
    Invalid(PlaygroundEvaluateError),
    /// The request does not conform to the schema it was given.
    NonConformant(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_request() -> PlaygroundAuthorizationRequest {
        create_test_request_with_action("read")
    }

    fn create_test_request_with_action(action: &str) -> PlaygroundAuthorizationRequest {
        PlaygroundAuthorizationRequest::new(
            kernel::Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            kernel::Hrn::action("api", action),
            kernel::Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        )
    }

    #[tokio::test]
    async fn test_evaluate_permit_policy() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["permit(principal, action, resource);".to_string()];

        let result = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await;
        assert!(result.is_ok());
        let outcome = result.unwrap();
        assert_eq!(outcome.decision, Decision::Allow);
    }

    #[tokio::test]
    async fn test_evaluate_forbid_policy() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["forbid(principal, action, resource);".to_string()];

        let result = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await;
        assert!(result.is_ok());
        let outcome = result.unwrap();
        assert_eq!(outcome.decision, Decision::Deny);
    }

    #[tokio::test]
    async fn test_evaluate_invalid_policy() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["invalid policy syntax".to_string()];

        let result = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await;
        assert!(result.is_err());
    }

    /// A schema that declares the action `read` for `Iam::User` principals
    /// and `Storage::Document` resources.
    ///
    /// It declares exactly the types the request uses: a schema that leaves a
    /// type out does not accept a request that names it.
    fn schema_declaring_read() -> Schema {
        let fragment = cedar_policy::SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
namespace Storage {
    entity Document {
        name: String
    };
}
action "read" appliesTo { principal: [Iam::User], resource: [Storage::Document] };
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    /// A schema that declares the action `read` in the namespace `Hodei`.
    fn schema_declaring_namespaced_read() -> Schema {
        let fragment = cedar_policy::SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
namespace Storage {
    entity Document {
        name: String
    };
}
namespace Hodei {
    action "read" appliesTo { principal: [Iam::User], resource: [Storage::Document] };
}
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    /// A schema that declares the action `read` for `Iam::User` principals
    /// and resources only: it does not know the `Storage::Document` type.
    fn schema_declaring_user_only() -> Schema {
        let fragment = cedar_policy::SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
action "read" appliesTo { principal: [Iam::User], resource: [Iam::User] };
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    #[tokio::test]
    async fn test_an_undeclared_action_is_denied() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request_with_action("write");
        let schema = schema_declaring_read();
        let policies = vec!["permit(principal, action, resource);".to_string()];

        let outcome = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await
            .expect("the evaluation must not fail");

        assert_eq!(
            outcome.decision,
            Decision::Deny,
            "an action the schema does not declare must not be authorized"
        );
        assert!(
            outcome.errors.iter().any(|e| e.contains("write")),
            "the reason must name the action: {:?}",
            outcome.errors
        );
    }

    #[tokio::test]
    async fn test_the_schema_is_the_authority_of_the_action_namespace() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        let policies =
            vec!["permit(principal, action == Hodei::Action::\"read\", resource);".to_string()];

        let outcome = evaluator
            .evaluate(
                &request,
                &policies,
                &schema_declaring_namespaced_read(),
                &HashMap::new(),
            )
            .await
            .expect("the evaluation must not fail");

        assert_eq!(
            outcome.decision,
            Decision::Allow,
            "the schema resolves the action to the namespaced form: {:?}",
            outcome.errors
        );
    }

    #[tokio::test]
    async fn test_a_request_that_does_not_conform_is_denied() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        // The schema does not declare the `Storage::Document` type, so the
        // request does not conform to it.
        let schema = schema_declaring_user_only();
        let policies = vec!["permit(principal, action, resource);".to_string()];

        let outcome = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await
            .expect("the evaluation must not fail");

        assert_eq!(
            outcome.decision,
            Decision::Deny,
            "a request that does not conform to the schema must be denied"
        );
        assert!(
            !outcome.errors.is_empty(),
            "the reason of the denial must be reported"
        );
    }

    #[tokio::test]
    async fn test_a_schema_that_declares_nothing_constrains_nothing() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let request = create_test_request();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let policies = vec!["permit(principal, action, resource);".to_string()];

        let outcome = evaluator
            .evaluate(&request, &policies, &schema, &HashMap::new())
            .await
            .expect("the evaluation must not fail");

        assert_eq!(
            outcome.decision,
            Decision::Allow,
            "an empty schema must not reject a request it knows nothing about"
        );
    }

    #[tokio::test]
    async fn test_parse_policies_empty() {
        let evaluator = PolicyEvaluatorAdapter::new();
        let result = evaluator.parse_policies(&[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().policies().count(), 0);
    }

    #[test]
    fn test_default_constructor() {
        let _evaluator = PolicyEvaluatorAdapter;
    }
}
