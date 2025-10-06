//! Test to verify that the domain modules compile correctly

#[cfg(test)]
mod tests {
    use policies::domain::HodeiEntityType;
    use policies::shared::application::EngineBuilder;

    // Tipos de prueba locales que representan entidades del dominio (ahora en IAM)
    struct TestUserType;
    struct TestGroupType;

    impl HodeiEntityType for TestUserType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "User" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![
                ("name", policies::domain::AttributeType::Primitive("String")),
                ("email", policies::domain::AttributeType::Primitive("String")),
            ]
        }
    }

    impl HodeiEntityType for TestGroupType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "Group" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![ ("name", policies::domain::AttributeType::Primitive("String")) ]
        }
    }

    #[test]
    fn test_user_entity_type() {
        assert_eq!(TestUserType::entity_type_name(), "User");
        // cedar_entity_type_name debe incluir el namespace en PascalCase
        let ty = TestUserType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::User");
    }

    #[test]
    fn test_group_entity_type() {
        assert_eq!(TestGroupType::entity_type_name(), "Group");
        let ty = TestGroupType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::Group");
    }

    #[test]
    fn test_user_cedar_attributes_present() {
        let attrs = TestUserType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestUserType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_group_cedar_attributes_present() {
        let attrs = TestGroupType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestGroupType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_engine_builder() {
        let _builder = EngineBuilder::new();
        // Just testing that we can create an engine builder
        assert!(true);
    }
}
