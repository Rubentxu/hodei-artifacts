use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use cedar_policy::{Schema, SchemaFragment};
use kernel::{AttributeValue, HodeiEntity};

/// Builds a Cedar schema from entity instances using DSL generation
///
/// This function generates Cedar schema DSL fragments based on the entity types
/// and attributes observed in the provided entity instances. It's inspired by
/// the schema_assembler.rs approach but adapted for runtime schema generation.
///
/// # Arguments
/// * `entities` - Slice of entity references to analyze for schema generation
///
/// # Returns
/// A Cedar `Schema` containing entity type definitions
///
/// # Implementation Notes
/// This is a simplified implementation that infers schema from entity instances.
/// A complete implementation would use `HodeiEntityType::attributes_schema()`
/// for type-level schema definitions rather than instance-level inference.
pub(crate) fn build_schema_from_entities(
    entities: &[&dyn HodeiEntity],
) -> Result<Schema, EvaluatePoliciesError> {
    use std::fmt::Write;

    let mut processed_types = std::collections::HashSet::new();
    let mut schema_fragments = Vec::new();

    // Process each entity to extract type information
    for entity in entities {
        let full_type_name = entity.hrn().entity_type_name();

        // Skip if we've already processed this type
        if processed_types.contains(&full_type_name) {
            continue;
        }
        processed_types.insert(full_type_name.clone());

        // Parse namespace and entity name (e.g., "Iam::User" -> namespace: "Iam", entity: "User")
        let parts: Vec<&str> = full_type_name.split("::").collect();
        if parts.len() != 2 {
            continue; // Skip malformed type names
        }

        let namespace = parts[0];
        let entity_name = parts[1];

        // Collect attributes from this entity instance
        let mut attributes = Vec::new();
        for (attr_name, attr_value) in entity.attributes() {
            let cedar_type = attribute_value_to_cedar_type(&attr_value);
            attributes.push((attr_name.as_str().to_string(), cedar_type));
        }

        // Generate Cedar DSL for this entity type
        let mut dsl = String::new();

        // Write namespace block
        writeln!(dsl, "namespace {} {{", namespace).unwrap();

        // Write entity definition
        writeln!(dsl, "    entity {} {{", entity_name).unwrap();

        // Write attributes
        for (i, (name, cedar_type)) in attributes.iter().enumerate() {
            if i < attributes.len() - 1 {
                writeln!(dsl, "        {}: {},", name, cedar_type).unwrap();
            } else {
                writeln!(dsl, "        {}: {}", name, cedar_type).unwrap();
            }
        }

        // Close entity and namespace
        writeln!(dsl, "    }};").unwrap();
        writeln!(dsl, "}}").unwrap();

        // Parse the DSL into a SchemaFragment
        match SchemaFragment::from_cedarschema_str(&dsl) {
            Ok((fragment, _warnings)) => {
                schema_fragments.push(fragment);
            }
            Err(e) => {
                // Log warning but continue with other fragments
                eprintln!(
                    "Warning: Failed to parse schema fragment for {}: {}",
                    full_type_name, e
                );
            }
        }
    }

    // Build the complete schema from fragments
    Schema::from_schema_fragments(schema_fragments)
        .map_err(|e| EvaluatePoliciesError::SchemaError(format!("Failed to build schema: {}", e)))
}

/// Converts an AttributeValue to its Cedar type representation
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
