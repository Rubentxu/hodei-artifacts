use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use cedar_policy::{Schema, SchemaFragment};
use kernel::{AttributeValue, HodeiEntity};
use std::collections::HashSet;
use std::fmt::Write;

/// Builds a Cedar schema from entity instances using the entities' type metadata
///
/// This function generates Cedar schema DSL fragments based on the entity types
/// observed in the provided entity instances, following the pattern from
/// the legacy policies crate schema_assembler.
#[allow(dead_code)]
pub(crate) fn build_schema_from_entities(
    entities: &[&dyn HodeiEntity],
) -> Result<Schema, EvaluatePoliciesError> {
    let mut processed_types = HashSet::new();
    let mut schema_fragments = Vec::new();

    // Process each entity to extract type information
    for entity in entities {
        let full_type_name = entity.hrn().entity_type_name();

        // Skip if we've already processed this type
        if processed_types.contains(&full_type_name) {
            continue;
        }
        processed_types.insert(full_type_name.clone());

        // Generate schema fragment for this entity type
        match generate_fragment_for_entity(*entity) {
            Ok(fragment) => {
                schema_fragments.push(fragment);
            }
            Err(e) => {
                // Log warning but continue with other fragments
                tracing::warn!(
                    "Failed to generate schema fragment for {}: {}",
                    full_type_name,
                    e
                );
            }
        }
    }

    // Build the complete schema from fragments
    Schema::from_schema_fragments(schema_fragments)
        .map_err(|e| EvaluatePoliciesError::SchemaError(format!("Failed to build schema: {}", e)))
}

/// Generate a Cedar SchemaFragment for a given entity instance
///
/// This function follows the pattern from the legacy policies crate schema_assembler
/// to generate proper schema fragments that include entity attributes.
#[allow(dead_code)]
fn generate_fragment_for_entity(
    entity: &dyn HodeiEntity,
) -> Result<SchemaFragment, EvaluatePoliciesError> {
    let full_type_name = entity.hrn().entity_type_name();

    // Parse namespace and entity name (e.g., "Iam::User" -> namespace: "Iam", entity: "User")
    let parts: Vec<&str> = full_type_name.split("::").collect();
    if parts.len() != 2 {
        return Err(EvaluatePoliciesError::SchemaError(format!(
            "Invalid entity type name: {}",
            full_type_name
        )));
    }

    let namespace = parts[0];
    let entity_name = parts[1];

    // Convert namespace to Pascal case (e.g., "iam" -> "Iam")
    let namespace_pascal =
        namespace.chars().next().unwrap().to_uppercase().to_string() + &namespace[1..];

    // Generate Cedar DSL for this entity type
    let mut dsl = String::new();

    // Write namespace block
    writeln!(dsl, "namespace {} {{", namespace_pascal).map_err(|e| {
        EvaluatePoliciesError::SchemaError(format!("Failed to write namespace: {}", e))
    })?;

    // Write entity definition with attributes
    writeln!(dsl, "    entity {} {{", entity_name).map_err(|e| {
        EvaluatePoliciesError::SchemaError(format!("Failed to write entity: {}", e))
    })?;

    // Add attributes based on the entity's attributes
    let attrs = entity.attributes();
    for (i, (name, value)) in attrs.iter().enumerate() {
        let cedar_type = attribute_value_to_cedar_type(value);
        if i < attrs.len() - 1 {
            writeln!(dsl, "        {}: {},", name.as_str(), cedar_type).map_err(|e| {
                EvaluatePoliciesError::SchemaError(format!("Failed to write attribute: {}", e))
            })?;
        } else {
            writeln!(dsl, "        {}: {}", name.as_str(), cedar_type).map_err(|e| {
                EvaluatePoliciesError::SchemaError(format!("Failed to write attribute: {}", e))
            })?;
        }
    }

    // Close entity and namespace
    writeln!(dsl, "    }};").map_err(|e| {
        EvaluatePoliciesError::SchemaError(format!("Failed to close entity: {}", e))
    })?;
    writeln!(dsl, "}}").map_err(|e| {
        EvaluatePoliciesError::SchemaError(format!("Failed to close namespace: {}", e))
    })?;

    // Parse the DSL into a SchemaFragment
    SchemaFragment::from_cedarschema_str(&dsl)
        .map_err(|e| {
            EvaluatePoliciesError::SchemaError(format!("Failed to parse schema fragment: {}", e))
        })
        .map(|(fragment, _warnings)| fragment)
}

/// Converts an AttributeValue to its Cedar type representation
#[allow(dead_code)]
fn attribute_value_to_cedar_type(value: &AttributeValue) -> String {
    match value {
        AttributeValue::String(_) => "String".to_string(),
        AttributeValue::Long(_) => "Long".to_string(),
        AttributeValue::Bool(_) => "Bool".to_string(),
        AttributeValue::Set(set) => {
            if let Some(first) = set.iter().next() {
                let element_type = attribute_value_to_cedar_type(first);
                format!("Set<{}>", element_type)
            } else {
                "Set<String>".to_string()
            }
        }
        AttributeValue::Record(_) => "Record".to_string(),
        AttributeValue::EntityRef(_) => "__cedar::Entity".to_string(),
    }
}
