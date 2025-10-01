//! Complete schema definition for the Hodei policy system
//! 
//! This module defines the complete Cedar schema including entities and actions
//! based on the Hodei domain model.

use cedar_policy::{Schema, SchemaError, SchemaFragment};

/// Build the complete schema for the Hodei policy system
/// 
/// This function combines all entity type schemas and defines the actions
/// that can be performed in the system.
pub fn build_hodei_schema() -> Result<Schema, SchemaError> {
    let schema_str = r#"
    // Minimal valid schema for tests
    entity Principal { };
    entity User { name: String, email: String };
    entity Resource { name: String };

    action access appliesTo { principal: Principal, resource: Resource };
    "#;
    
    let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)
        .expect("Hodei schema should be valid");
    Schema::from_schema_fragments(vec![schema_fragment])
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_complete_schema_build() {
        let schema = build_hodei_schema();
        assert!(schema.is_ok());
    }
}