use crate::shared::domain::HodeiEntity;
use crate::shared::domain::ports::{ActionTrait, Principal, Resource};
use crate::shared::generate_fragment_for_type;
use cedar_policy::{CedarSchemaError, Schema, SchemaError, SchemaFragment};

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

    pub fn register_action<A: ActionTrait>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let (principal_type, resource_type) = A::applies_to();
        let schema_str = format!(
            "action \"{}\" appliesTo {{ principal: {}, resource: {} }};",
            A::name(),
            principal_type,
            resource_type
        );

        // Parse the action schema fragment
        let (frag, _warnings) =
            SchemaFragment::from_cedarschema_str(&schema_str).map_err(|_e| {
                // If parsing fails, create a SchemaError by parsing an intentionally invalid schema
                // This ensures we return the correct error type
                let invalid = "entity Invalid { invalid: Invalid }";
                match SchemaFragment::from_cedarschema_str(invalid) {
                    Ok(_) => unreachable!(),
                    Err(_cedar_err) => {
                        // Create a generic schema parsing error using Schema::from_schema_fragments
                        // with an empty fragment list to trigger a schema error
                        Box::new(CedarSchemaError::from(
                            Schema::from_schema_fragments(vec![]).unwrap_err(),
                        ))
                    }
                }
            })?;

        self.action_fragments.push(frag);
        Ok(self)
    }

    pub fn build_schema(self) -> Result<Schema, Box<SchemaError>> {
        // Build schema from registered fragments only
        // No automatic base schema - everything must be explicitly registered by the client
        let all_fragments = [self.entity_fragments, self.action_fragments].concat();
        Schema::from_schema_fragments(all_fragments).map_err(Box::new)
    }
}
