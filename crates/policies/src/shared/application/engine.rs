use crate::shared::application::PolicyStore;
use crate::shared::domain::ports::{Action, Principal, Resource};
use crate::shared::domain::{HodeiEntity, PolicyStorage};
use crate::shared::generate_fragment_for_type;
use cedar_policy::{CedarSchemaError, Context, Entities, PolicySet, Request, Response, Schema, SchemaError, SchemaFragment};
use std::collections::HashSet;
use std::sync::Arc;

pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: cedar_policy::EntityUid,
    pub resource: &'a dyn HodeiEntity,
    pub context: Context,
    pub entities: Vec<&'a dyn HodeiEntity>,
}

#[derive(Clone)]
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response {
        let entity_vec: Vec<cedar_policy::Entity> = request
            .entities
            .iter()
            .map(|entity| {
                let attrs = entity.attributes();
                let parents: HashSet<_> = entity.parents().into_iter().collect();
                cedar_policy::Entity::new(entity.euid(), attrs, parents)
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to create entities");

        let entities = Entities::from_entities(entity_vec, None)
            .expect("Failed to create Entities collection");

        let cedar_request = Request::new(
            request.principal.euid(),
            request.action.clone(),
            request.resource.euid(),
            request.context.clone(),
            None,
        )
        .expect("Failed to create Cedar request");

        let policies = self
            .store
            .get_current_policy_set()
            .await
            .unwrap_or_else(|_| PolicySet::new());
        cedar_policy::Authorizer::new().is_authorized(&cedar_request, &policies, &entities)
    }
}

#[derive(Default)]
pub struct EngineBuilder {
    entity_fragments: Vec<SchemaFragment>,
    action_fragments: Vec<SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self { 
        Self::default() 
    }

    // New methods for the generic approach
    pub fn register_principal<P: Principal>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<P>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_resource<R: Resource>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<R>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_action<A: Action>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let (principal_type, resource_type) = A::applies_to();
        let schema_str = format!(
            "action \"{}\" appliesTo {{ principal: {}, resource: {} }};",
            A::name(), principal_type, resource_type
        );

        // Parse the action schema fragment
        let (frag, _warnings) = SchemaFragment::from_cedarschema_str(&schema_str)
            .map_err(|_e| {
                // If parsing fails, create a SchemaError by parsing an intentionally invalid schema
                // This ensures we return the correct error type
                let invalid = "entity Invalid { invalid: Invalid }";
                match SchemaFragment::from_cedarschema_str(invalid) {
                    Ok(_) => unreachable!(),
                    Err(_cedar_err) => {
                        // Create a generic schema parsing error using Schema::from_schema_fragments
                        // with an empty fragment list to trigger a schema error
                        Box::new(CedarSchemaError::from(
                            Schema::from_schema_fragments(vec![]).unwrap_err()
                        ))
                    }
                }
            })?;

        self.action_fragments.push(frag);
        Ok(self)
    }

    pub fn build(
        self,
        storage: Arc<dyn PolicyStorage>,
    ) -> Result<(AuthorizationEngine, PolicyStore), Box<SchemaError>> {
        // Build schema from registered fragments only
        // No automatic base schema - everything must be explicitly registered by the client
        let all_fragments = [self.entity_fragments, self.action_fragments].concat();
        
        let schema = Arc::new(Schema::from_schema_fragments(all_fragments)?);
        let store = PolicyStore::new(schema.clone(), storage);
        let engine = AuthorizationEngine { 
            schema, 
            store: store.clone() 
        };
        Ok((engine, store))
    }
}