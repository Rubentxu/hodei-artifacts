//! Test file to verify schema implementation works correctly

#[cfg(test)]
mod tests {
    use crate::domain::schema::build_hodei_schema;
    use crate::domain::{HodeiEntityType, User, Group, ServiceAccount, Namespace};
    use crate::domain::schema::build_principal_schema_fragment;
    use cedar_policy::Schema;

    #[test]
    fn test_complete_schema_build() {
        let schema = build_hodei_schema();
        assert!(schema.is_ok(), "Failed to build complete schema: {:?}", schema.err());
    }

    #[test]
    fn test_principal_schema_fragments() {
        let user_fragment = build_principal_schema_fragment::<User>();
        assert!(user_fragment.is_ok(), "Failed to build User schema fragment: {:?}", user_fragment.err());
        
        let group_fragment = build_principal_schema_fragment::<Group>();
        assert!(group_fragment.is_ok(), "Failed to build Group schema fragment: {:?}", group_fragment.err());
        
        let service_account_fragment = build_principal_schema_fragment::<ServiceAccount>();
        assert!(service_account_fragment.is_ok(), "Failed to build ServiceAccount schema fragment: {:?}", service_account_fragment.err());
        
        let namespace_fragment = build_principal_schema_fragment::<Namespace>();
        assert!(namespace_fragment.is_ok(), "Failed to build Namespace schema fragment: {:?}", namespace_fragment.err());
    }

    #[test]
    fn test_schema_validation() {
        let schema = build_hodei_schema().expect("Failed to build schema");
        
        // Test that we can create a validator with the schema
        let _validator = cedar_policy::Validator::new(schema);
    }
}