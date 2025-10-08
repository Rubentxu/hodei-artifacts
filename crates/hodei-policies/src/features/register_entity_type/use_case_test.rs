#[cfg(test)]
mod tests {
    use super::super::use_case::RegisterEntityTypeUseCase;
    use crate::internal::engine::builder::EngineBuilder;
    use kernel::{AttributeName, AttributeType, HodeiEntityType, ResourceTypeName, ServiceName};
    use std::sync::{Arc, Mutex};

    // Mock entity types for testing

    struct MockUser;

    impl HodeiEntityType for MockUser {
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
                (AttributeName::new("name").unwrap(), AttributeType::String),
                (AttributeName::new("active").unwrap(), AttributeType::Bool),
                (AttributeName::new("role").unwrap(), AttributeType::String),
            ]
        }
    }

    struct MockDocument;

    impl HodeiEntityType for MockDocument {
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
                (AttributeName::new("title").unwrap(), AttributeType::String),
                (
                    AttributeName::new("classification").unwrap(),
                    AttributeType::String,
                ),
                (AttributeName::new("owner").unwrap(), AttributeType::String),
            ]
        }
    }

    struct MockGroup;

    impl HodeiEntityType for MockGroup {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Group").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (AttributeName::new("name").unwrap(), AttributeType::String),
                (
                    AttributeName::new("members").unwrap(),
                    AttributeType::Set(Box::new(AttributeType::String)),
                ),
            ]
        }
    }

    fn create_use_case() -> RegisterEntityTypeUseCase {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        RegisterEntityTypeUseCase::new(builder)
    }

    #[test]
    fn test_register_single_entity_type() {
        let use_case = create_use_case();

        let result = use_case.register::<MockUser>();

        assert!(
            result.is_ok(),
            "Failed to register entity type: {:?}",
            result
        );
        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[test]
    fn test_register_multiple_entity_types() {
        let use_case = create_use_case();

        let result1 = use_case.register::<MockUser>();
        let result2 = use_case.register::<MockDocument>();
        let result3 = use_case.register::<MockGroup>();

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
        assert_eq!(use_case.count().unwrap(), 3);
    }

    #[test]
    fn test_register_duplicate_entity_type_is_idempotent() {
        let use_case = create_use_case();

        let result1 = use_case.register::<MockUser>();
        let result2 = use_case.register::<MockUser>();
        let result3 = use_case.register::<MockUser>();

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
        // Should only be registered once
        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[test]
    fn test_clear_registered_entity_types() {
        let use_case = create_use_case();

        use_case.register::<MockUser>().unwrap();
        use_case.register::<MockDocument>().unwrap();

        assert_eq!(use_case.count().unwrap(), 2);

        let clear_result = use_case.clear();
        assert!(clear_result.is_ok());
        assert_eq!(use_case.count().unwrap(), 0);
    }

    #[test]
    fn test_clear_empty_use_case() {
        let use_case = create_use_case();

        assert_eq!(use_case.count().unwrap(), 0);

        let clear_result = use_case.clear();
        assert!(clear_result.is_ok());
        assert_eq!(use_case.count().unwrap(), 0);
    }

    #[test]
    fn test_register_after_clear() {
        let use_case = create_use_case();

        use_case.register::<MockUser>().unwrap();
        assert_eq!(use_case.count().unwrap(), 1);

        use_case.clear().unwrap();
        assert_eq!(use_case.count().unwrap(), 0);

        use_case.register::<MockDocument>().unwrap();
        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[test]
    fn test_count_with_no_registrations() {
        let use_case = create_use_case();

        let count = use_case.count();

        assert!(count.is_ok());
        assert_eq!(count.unwrap(), 0);
    }

    #[test]
    fn test_entity_type_name_format() {
        // Verify that the entity type names are correctly formatted
        assert_eq!(MockUser::entity_type_name(), "Iam::User");
        assert_eq!(MockDocument::entity_type_name(), "Storage::Document");
        assert_eq!(MockGroup::entity_type_name(), "Iam::Group");
    }

    #[test]
    fn test_principal_types() {
        assert!(MockUser::is_principal_type());
        assert!(MockGroup::is_principal_type());
        assert!(!MockDocument::is_principal_type());
    }

    #[test]
    fn test_shared_builder_across_multiple_use_cases() {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case1 = RegisterEntityTypeUseCase::new(builder.clone());
        let use_case2 = RegisterEntityTypeUseCase::new(builder.clone());

        use_case1.register::<MockUser>().unwrap();
        use_case2.register::<MockDocument>().unwrap();

        // Both use cases should see the same builder state
        assert_eq!(use_case1.count().unwrap(), 2);
        assert_eq!(use_case2.count().unwrap(), 2);
    }

    #[test]
    fn test_attributes_schema_defined() {
        let user_schema = MockUser::attributes_schema();
        assert!(!user_schema.is_empty());
        assert_eq!(user_schema.len(), 3);

        let doc_schema = MockDocument::attributes_schema();
        assert!(!doc_schema.is_empty());
        assert_eq!(doc_schema.len(), 3);
    }
}
