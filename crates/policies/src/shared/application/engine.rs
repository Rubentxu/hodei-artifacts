use crate::domain::actions;
use crate::domain::{HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStore};
use crate::shared::domain::ports::{Action, Principal, Resource};
use crate::shared::generate_fragment_for_type;
use cedar_policy::{
    Context, Entities, PolicySet, Request, Response, Schema, SchemaError, SchemaFragment,
};
use std::collections::{HashMap, HashSet};
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
    // Keep the old fields for backward compatibility
    partials: HashMap<&'static str, SchemaFragment>,
    // New fields for the generic approach
    entity_fragments: Vec<SchemaFragment>,
    action_fragments: Vec<SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self { 
        Self::default() 
    }

    // Old method for backward compatibility
    pub fn register_entity_type<T: HodeiEntityType + 'static>(
        &mut self,
    ) -> Result<&mut Self, Box<SchemaError>> {
        let frag = generate_fragment_for_type::<T>()?;
        self.partials.insert(T::entity_type_name(), frag);
        Ok(self)
    }

    // New methods for the generic approach
    pub fn register_principal<P: Principal>(&mut self) -> Result<&mut Self, Box<SchemaError>> {
        let frag = generate_fragment_for_type::<P>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_resource<R: Resource>(&mut self) -> Result<&mut Self, Box<SchemaError>> {
        let frag = generate_fragment_for_type::<R>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_action<A: Action>(&mut self) -> Result<&mut Self, Box<SchemaError>> {
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
                    Err(cedar_err) => {
                        // Create a generic schema parsing error using Schema::from_schema_fragments
                        // with an empty fragment list to trigger a schema error
                        Box::new(SchemaError::from(
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
        // If we have new fragments, use them; otherwise, fall back to the old approach
        if !self.entity_fragments.is_empty() || !self.action_fragments.is_empty() {
            let all_fragments = [self.entity_fragments, self.action_fragments].concat();
            let schema = Arc::new(Schema::from_schema_fragments(all_fragments)?);
            let store = PolicyStore::new(schema.clone(), storage);
            let engine = AuthorizationEngine { 
                schema, 
                store: store.clone() 
            };
            Ok((engine, store))
        } else {
            // Compose schema from base + registered partials + feature actions
            // Base provides fundamental types referenced by partials/actions
            let base = r#"
            entity Principal { };
            entity Resource { name: String };
            "#;
            let (base_frag, _) =
                SchemaFragment::from_cedarschema_str(base).expect("Base schema should be valid");

            let mut fragments: Vec<SchemaFragment> = Vec::new();
            fragments.push(base_frag);
            let has_partials = !self.partials.is_empty();
            fragments.extend(self.partials.into_values());

            // Add actions derived from feature directories only when there are registered partials
            if !fragments.is_empty() && has_partials {
                let actions_frag = actions::build_feature_actions_fragment()
                    .expect("actions fragment should be valid");
                fragments.push(actions_frag);
            }

            let schema = Arc::new(Schema::from_schema_fragments(fragments)?);

            let store = PolicyStore::new(schema.clone(), storage);
            let engine = AuthorizationEngine {
                schema,
                store: store.clone(),
            };
            Ok((engine, store))
        }
    }
}