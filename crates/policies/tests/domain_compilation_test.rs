//! Test to verify that the domain modules compile correctly

#[cfg(test)]
mod tests {
    use policies::domain::{HodeiEntity, HodeiEntityType, AuthorizationEngine, EngineBuilder};
    use policies::domain::principals::{User, Group};
    use cedar_policy::{SchemaFragment, SchemaError};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_user_entity_type() {
        assert_eq!(User::entity_type_name(), "User");
    }

    #[test]
    fn test_group_entity_type() {
        assert_eq!(Group::entity_type_name(), "Group");
    }

    #[test]
    fn test_user_partial_schema() {
        let schema = User::partial_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn test_group_partial_schema() {
        let schema = Group::partial_schema();
        assert!(schema.is_ok());
    }

    #[test]
    fn test_engine_builder() {
        let builder = EngineBuilder::new();
        // Just testing that we can create an engine builder
        assert!(true);
    }
}