//! Engine Schema Builder
//!
//! This module provides a builder for creating Cedar schemas from registered types.
//! It's focused on schema generation only, leaving evaluation to Cedar directly.

use cedar_policy::{CedarSchemaError, Schema, SchemaError, SchemaFragment};
use kernel::{HodeiEntity, HodeiEntityType};
use std::collections::HashMap;

// ============================================================================
// Schema Builder Types
// ============================================================================

/// Schema builder for creating Cedar schemas from entity types
///
/// This builder allows registering entity types and actions to generate
/// a complete Cedar schema that can be used for policy evaluation.
#[derive(Default)]
#[allow(dead_code)]
pub struct EngineBuilder {
    /// Entity schema fragments
    entity_fragments: HashMap<String, SchemaFragment>,
    /// Action schema fragments
    action_fragments: Vec<SchemaFragment>,
    /// Cache for generated fragments
    #[allow(dead_code)]
    fragment_cache: HashMap<String, SchemaFragment>,
}

impl EngineBuilder {
    /// Create a new schema builder
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an entity type for schema generation
    ///
    /// This method generates a schema fragment for the entity type `T`
    /// and caches it for later use in schema building.
    #[allow(dead_code)]
    pub fn register_entity<T: HodeiEntityType>(
        &mut self,
    ) -> Result<&mut Self, Box<CedarSchemaError>> {
        let type_name = T::entity_type_name();

        // Check if already registered
        if self.entity_fragments.contains_key(&type_name) {
            return Ok(self);
        }

        // Generate schema fragment for this type
        let fragment = generate_fragment_for_type::<T>()?;
        self.entity_fragments.insert(type_name, fragment);
        Ok(self)
    }

    /// Register an action for schema generation using ActionTrait
    ///
    /// This generates an action schema fragment dynamically from the ActionTrait implementation.
    #[allow(dead_code)]
    pub fn register_action_type<A: kernel::ActionTrait>(
        &mut self,
    ) -> Result<&mut Self, Box<CedarSchemaError>> {
        let fragment = generate_action_fragment::<A>()?;
        self.action_fragments.push(fragment);
        Ok(self)
    }

    /// Register an entity instance for schema generation
    ///
    /// This method generates schema fragments based on entity instances
    /// by extracting their type information and attributes.
    #[allow(dead_code)]
    pub fn register_entity_instance(
        &mut self,
        entity: &dyn HodeiEntity,
    ) -> Result<&mut Self, Box<CedarSchemaError>> {
        let type_name = entity.hrn().entity_type_name();

        // Check if already registered
        if self.entity_fragments.contains_key(&type_name) {
            return Ok(self);
        }

        // Generate schema fragment based on entity instance
        let fragment = generate_fragment_for_entity(entity)?;
        self.entity_fragments.insert(type_name, fragment);
        Ok(self)
    }

    /// Build the complete Cedar schema
    ///
    /// Combines all registered entity and action fragments into a single schema.
    #[allow(dead_code)]
    pub fn build_schema(self) -> Result<Schema, Box<SchemaError>> {
        let all_fragments: Vec<SchemaFragment> = self
            .entity_fragments
            .into_values()
            .chain(self.action_fragments)
            .collect();

        Schema::from_schema_fragments(all_fragments).map_err(Box::new)
    }

    /// Get the number of registered entity types
    #[allow(dead_code)]
    pub fn entity_count(&self) -> usize {
        self.entity_fragments.len()
    }

    /// Get the number of registered actions
    #[allow(dead_code)]
    pub fn action_count(&self) -> usize {
        self.action_fragments.len()
    }

    /// Clear all registered types and fragments
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.entity_fragments.clear();
        self.action_fragments.clear();
        self.fragment_cache.clear();
    }
}

// ============================================================================
// Schema Generation Functions
// ============================================================================

/// Generate a Cedar schema fragment for a given entity type
///
/// This function follows the pattern from the legacy policies crate
/// to generate proper schema fragments that include entity attributes.
#[allow(dead_code)]
fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<CedarSchemaError>>
{
    let type_name = T::entity_type_name();

    // Parse namespace and entity name (e.g., "Iam::User" -> namespace: "Iam", entity: "User")
    let parts: Vec<&str> = type_name.split("::").collect();
    if parts.len() != 2 {
        // Create a schema error by attempting to parse invalid schema
        return Err(Box::new(CedarSchemaError::from(
            Schema::from_schema_fragments(vec![]).expect_err("Expected schema error"),
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
    dsl.push_str(&format!("namespace {} {{\n", namespace_pascal));

    // Write entity definition with attributes
    dsl.push_str(&format!("    entity {} {{\n", entity_name));

    // Add attributes based on the entity's schema
    let attrs = T::attributes_schema();
    for (i, (name, atype)) in attrs.iter().enumerate() {
        if i < attrs.len() - 1 {
            dsl.push_str(&format!(
                "        {}: {},\n",
                name.as_str(),
                to_cedar_type(atype)
            ));
        } else {
            dsl.push_str(&format!(
                "        {}: {}\n",
                name.as_str(),
                to_cedar_type(atype)
            ));
        }
    }

    // Close entity and namespace
    dsl.push_str("    };\n");
    dsl.push_str("}\n");

    // Parse the DSL into a SchemaFragment
    SchemaFragment::from_cedarschema_str(&dsl)
        .map_err(Box::new)
        .map(|(fragment, _warnings)| fragment)
}

/// Generate a Cedar schema fragment for a given entity instance
///
/// This function generates schema fragments based on entity instances
/// by extracting their type information and attributes.
#[allow(dead_code)]
fn generate_fragment_for_entity(
    entity: &dyn HodeiEntity,
) -> Result<SchemaFragment, Box<CedarSchemaError>> {
    let type_name = entity.hrn().entity_type_name();

    // Parse namespace and entity name
    let parts: Vec<&str> = type_name.split("::").collect();
    if parts.len() != 2 {
        // Create a schema error by attempting to parse invalid schema
        return Err(Box::new(CedarSchemaError::from(
            Schema::from_schema_fragments(vec![]).expect_err("Expected schema error"),
        )));
    }

    let namespace = parts[0];
    let entity_name = parts[1];

    // Convert namespace to Pascal case
    let namespace_pascal =
        namespace.chars().next().unwrap().to_uppercase().to_string() + &namespace[1..];

    // Generate Cedar DSL for this entity type
    let mut dsl = String::new();

    // Write namespace block
    dsl.push_str(&format!("namespace {} {{\n", namespace_pascal));

    // Write entity definition with attributes
    dsl.push_str(&format!("    entity {} {{\n", entity_name));

    // Add attributes based on the entity's schema
    if let Some(attrs) = entity.cedar_attributes() {
        for (i, (name, atype)) in attrs.iter().enumerate() {
            if i < attrs.len() - 1 {
                dsl.push_str(&format!("        {}: {},\n", name, atype.to_cedar_decl()));
            } else {
                dsl.push_str(&format!("        {}: {}\n", name, atype.to_cedar_decl()));
            }
        }
    }

    // Close entity and namespace
    dsl.push_str("    };\n");
    dsl.push_str("}\n");

    // Parse the DSL into a SchemaFragment
    SchemaFragment::from_cedarschema_str(&dsl)
        .map_err(Box::new)
        .map(|(fragment, _warnings)| fragment)
}

/// Generate a Cedar schema fragment for an action using ActionTrait
///
/// This generates schema fragments dynamically from action types that implement ActionTrait.
#[allow(dead_code)]
pub fn generate_action_fragment<A: kernel::ActionTrait>()
-> Result<SchemaFragment, Box<CedarSchemaError>> {
    let action_name = A::name();
    let principal_type = A::applies_to_principal();
    let resource_type = A::applies_to_resource();

    let actions_dsl = format!(
        r#"action "{}" appliesTo {{
            principal: [{}],
            resource: [{}]
        }};"#,
        action_name, principal_type, resource_type
    );

    SchemaFragment::from_cedarschema_str(&actions_dsl)
        .map_err(Box::new)
        .map(|(fragment, _warnings)| fragment)
}

/// Convert kernel AttributeType to Cedar type string
#[allow(dead_code)]
fn to_cedar_type(attr_type: &kernel::domain::AttributeType) -> &'static str {
    use kernel::domain::AttributeType;

    match attr_type {
        AttributeType::Bool => "Bool",
        AttributeType::Long => "Long",
        AttributeType::String => "String",
        AttributeType::Set(_) => "Set<String>", // Simplified for now
        AttributeType::Record(_) => "Record",   // Simplified for now
        AttributeType::EntityRef(_) => "__cedar::Entity", // Simplified for now
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{AttributeName, AttributeType, ResourceTypeName, ServiceName};
    use kernel::{ActionTrait, HodeiEntity, HodeiEntityType};
    use std::collections::HashMap;

    // ============================================================================
    // Test Entities
    // ============================================================================

    #[derive(Debug)]
    struct TestUser {
        hrn: kernel::Hrn,
        name: String,
        role: String,
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
            vec![
                (AttributeName::new("name").unwrap(), AttributeType::string()),
                (AttributeName::new("role").unwrap(), AttributeType::string()),
            ]
        }
    }

    impl HodeiEntity for TestUser {
        fn hrn(&self) -> &kernel::Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, kernel::AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("name").unwrap(),
                kernel::AttributeValue::string(&self.name),
            );
            attrs.insert(
                AttributeName::new("role").unwrap(),
                kernel::AttributeValue::string(&self.role),
            );
            attrs
        }
    }

    #[derive(Debug)]
    struct TestDocument {
        hrn: kernel::Hrn,
        title: String,
        owner: String,
    }

    impl HodeiEntityType for TestDocument {
        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Document").unwrap()
        }

        fn is_principal_type() -> bool {
            false
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (
                    AttributeName::new("title").unwrap(),
                    AttributeType::string(),
                ),
                (
                    AttributeName::new("owner").unwrap(),
                    AttributeType::string(),
                ),
            ]
        }
    }

    impl HodeiEntity for TestDocument {
        fn hrn(&self) -> &kernel::Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, kernel::AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("title").unwrap(),
                kernel::AttributeValue::string(&self.title),
            );
            attrs.insert(
                AttributeName::new("owner").unwrap(),
                kernel::AttributeValue::string(&self.owner),
            );
            attrs
        }
    }

    // ============================================================================
    // Test Actions
    // ============================================================================

    struct ReadAction;

    impl ActionTrait for ReadAction {
        fn name() -> &'static str {
            "Read"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Storage::Document".to_string()
        }
    }

    struct WriteAction;

    impl ActionTrait for WriteAction {
        fn name() -> &'static str {
            "Write"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Storage::Document".to_string()
        }
    }

    struct DeleteAction;

    impl ActionTrait for DeleteAction {
        fn name() -> &'static str {
            "Delete"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Storage::Document".to_string()
        }
    }

    // ============================================================================
    // Builder Basic Tests
    // ============================================================================

    #[test]
    fn create_builder() {
        let builder = EngineBuilder::new();
        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    #[test]
    fn builder_default() {
        let builder = EngineBuilder::default();
        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    // ============================================================================
    // Entity Registration Tests
    // ============================================================================

    #[test]
    fn register_entity_type() {
        let mut builder = EngineBuilder::new();
        let result = builder.register_entity::<TestUser>();
        assert!(result.is_ok());
        assert_eq!(builder.entity_count(), 1);
    }

    #[test]
    fn register_multiple_entity_types() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        assert_eq!(builder.entity_count(), 2);
    }

    #[test]
    fn register_duplicate_entity_type() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        let result = builder.register_entity::<TestUser>();
        assert!(result.is_ok());
        assert_eq!(builder.entity_count(), 1); // Should not increase
    }

    #[test]
    fn register_entity_instance() {
        let user = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let mut builder = EngineBuilder::new();
        let result = builder.register_entity_instance(&user);
        assert!(result.is_ok());
        assert_eq!(builder.entity_count(), 1);
    }

    #[test]
    fn register_multiple_entity_instances() {
        let user = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let doc = TestDocument {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "storage".to_string(),
                "123".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
            title: "Test Doc".to_string(),
            owner: "alice".to_string(),
        };

        let mut builder = EngineBuilder::new();
        builder.register_entity_instance(&user).unwrap();
        builder.register_entity_instance(&doc).unwrap();
        assert_eq!(builder.entity_count(), 2);
    }

    #[test]
    fn register_duplicate_entity_instance() {
        let user1 = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let user2 = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "bob".to_string(),
            ),
            name: "Bob".to_string(),
            role: "user".to_string(),
        };

        let mut builder = EngineBuilder::new();
        builder.register_entity_instance(&user1).unwrap();
        builder.register_entity_instance(&user2).unwrap();
        // Same type, should not increase count
        assert_eq!(builder.entity_count(), 1);
    }

    // ============================================================================
    // Action Registration Tests
    // ============================================================================

    #[test]
    fn register_action_type() {
        let mut builder = EngineBuilder::new();
        let result = builder.register_action_type::<ReadAction>();
        assert!(result.is_ok());
        assert_eq!(builder.action_count(), 1);
    }

    #[test]
    fn register_multiple_action_types() {
        let mut builder = EngineBuilder::new();
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();
        builder.register_action_type::<DeleteAction>().unwrap();
        assert_eq!(builder.action_count(), 3);
    }

    #[test]
    fn register_mixed_entities_and_actions() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();

        assert_eq!(builder.entity_count(), 2);
        assert_eq!(builder.action_count(), 2);
    }

    // ============================================================================
    // Schema Building Tests
    // ============================================================================

    #[test]
    fn build_schema_with_entity() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn build_schema_with_multiple_entities() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn build_schema_with_action() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        builder.register_action_type::<ReadAction>().unwrap();
        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn build_schema_with_multiple_actions() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();
        builder.register_action_type::<DeleteAction>().unwrap();
        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn build_schema_empty() {
        let builder = EngineBuilder::new();
        let schema = builder.build_schema();
        // Empty schema should build successfully
        assert!(schema.is_ok());
    }

    #[test]
    fn build_schema_consumes_builder() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        let _schema = builder.build_schema();
        // Builder is consumed, can't use it anymore
    }

    // ============================================================================
    // Clear Tests
    // ============================================================================

    #[test]
    fn clear_builder_with_entities() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();

        assert_eq!(builder.entity_count(), 2);

        builder.clear();

        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    #[test]
    fn clear_builder_with_actions() {
        let mut builder = EngineBuilder::new();
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();

        assert_eq!(builder.action_count(), 2);

        builder.clear();

        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    #[test]
    fn clear_builder_with_entities_and_actions() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();

        assert_eq!(builder.entity_count(), 2);
        assert_eq!(builder.action_count(), 2);

        builder.clear();

        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    #[test]
    fn clear_empty_builder() {
        let mut builder = EngineBuilder::new();
        builder.clear();
        assert_eq!(builder.entity_count(), 0);
        assert_eq!(builder.action_count(), 0);
    }

    #[test]
    fn reuse_builder_after_clear() {
        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.clear();
        builder.register_entity::<TestDocument>().unwrap();

        assert_eq!(builder.entity_count(), 1);
    }

    // ============================================================================
    // Fragment Generation Tests
    // ============================================================================

    #[test]
    fn generate_fragment_for_type_test() {
        let fragment = generate_fragment_for_type::<TestUser>();
        assert!(fragment.is_ok());
    }

    #[test]
    fn generate_fragment_for_multiple_types() {
        let fragment1 = generate_fragment_for_type::<TestUser>();
        let fragment2 = generate_fragment_for_type::<TestDocument>();
        assert!(fragment1.is_ok());
        assert!(fragment2.is_ok());
    }

    #[test]
    fn generate_action_fragment_test() {
        let fragment = generate_action_fragment::<ReadAction>();
        assert!(fragment.is_ok());
    }

    #[test]
    fn generate_multiple_action_fragments() {
        let fragment1 = generate_action_fragment::<ReadAction>();
        let fragment2 = generate_action_fragment::<WriteAction>();
        let fragment3 = generate_action_fragment::<DeleteAction>();
        assert!(fragment1.is_ok());
        assert!(fragment2.is_ok());
        assert!(fragment3.is_ok());
    }

    #[test]
    fn generate_fragment_for_entity_instance() {
        let user = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let fragment = generate_fragment_for_entity(&user);
        assert!(fragment.is_ok());
    }

    // ============================================================================
    // Integration Tests
    // ============================================================================

    #[test]
    fn full_schema_workflow() {
        let mut builder = EngineBuilder::new();

        // Register entities
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity::<TestDocument>().unwrap();

        // Register actions
        builder.register_action_type::<ReadAction>().unwrap();
        builder.register_action_type::<WriteAction>().unwrap();

        // Build schema
        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn schema_with_entity_instances() {
        let user = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let doc = TestDocument {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "storage".to_string(),
                "123".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
            title: "Test Doc".to_string(),
            owner: "alice".to_string(),
        };

        let mut builder = EngineBuilder::new();
        builder.register_entity_instance(&user).unwrap();
        builder.register_entity_instance(&doc).unwrap();
        builder.register_action_type::<ReadAction>().unwrap();

        let schema = builder.build_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn mixed_entity_registration() {
        let user_instance = TestUser {
            hrn: kernel::Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            name: "Alice".to_string(),
            role: "admin".to_string(),
        };

        let mut builder = EngineBuilder::new();
        builder.register_entity::<TestUser>().unwrap();
        builder.register_entity_instance(&user_instance).unwrap();
        builder.register_entity::<TestDocument>().unwrap();

        // Should still be 2 (User and Document)
        assert_eq!(builder.entity_count(), 2);
    }
}
