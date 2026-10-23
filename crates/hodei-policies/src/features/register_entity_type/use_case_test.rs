#[cfg(test)]
mod tests {
    use super::super::dto::{EntityAttribute, RegisterEntityTypeCommand};
    use super::super::error::RegisterEntityTypeError;
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

    // -----------------------------------------------------------------
    // The command-based path
    //
    // `execute` is the entry point the ports expose, so it is the one that
    // has to produce a real Cedar fragment. A command that registers nothing
    // but reports success would leave the schema empty while every caller
    // believes the type was declared.
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_execute_registers_the_entity_type_of_the_command() {
        let use_case = create_use_case();

        let command = RegisterEntityTypeCommand::with_attributes(
            "Iam::User".to_string(),
            vec![
                EntityAttribute::new("name".to_string(), "String".to_string()),
                EntityAttribute::new("active".to_string(), "Bool".to_string()),
            ],
        );

        use_case
            .execute(command)
            .await
            .expect("a qualified entity type must be registrable");

        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_execute_registers_a_schema_that_cedar_accepts() {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case = RegisterEntityTypeUseCase::new(builder.clone());

        use_case
            .execute(RegisterEntityTypeCommand::with_attributes(
                "Iam::User".to_string(),
                vec![
                    EntityAttribute::new("name".to_string(), "String".to_string()),
                    EntityAttribute::new("tags".to_string(), "Set<String>".to_string()),
                ],
            ))
            .await
            .expect("the entity type must be registrable");

        // The fragment has to survive Cedar's own schema construction —
        // otherwise the registration only moved a broken fragment around.
        // `build_schema` consumes the builder, so it is taken out of the
        // lock and replaced by an empty one, exactly as the build-schema use
        // case does.
        let schema = {
            let mut locked = builder.lock().expect("builder lock");
            std::mem::replace(&mut *locked, EngineBuilder::new())
        }
        .build_schema()
        .expect("the registered fragment must produce a valid Cedar schema");

        assert_eq!(
            schema.entity_types().count(),
            1,
            "the built schema must declare exactly the registered entity type"
        );
    }

    #[tokio::test]
    async fn test_execute_is_idempotent_for_the_same_entity_type() {
        let use_case = create_use_case();

        let command = RegisterEntityTypeCommand::new("Iam::User".to_string());

        use_case.execute(command.clone()).await.unwrap();
        use_case.execute(command.clone()).await.unwrap();

        assert_eq!(
            use_case.count().unwrap(),
            1,
            "registering the same entity type twice must keep one fragment"
        );
    }

    #[tokio::test]
    async fn test_execute_rejects_an_entity_type_without_a_namespace() {
        let use_case = create_use_case();

        let result = use_case
            .execute(RegisterEntityTypeCommand::new("User".to_string()))
            .await;

        match result {
            Err(RegisterEntityTypeError::InvalidEntityType(_)) => {}
            other => panic!("expected InvalidEntityType, got: {other:?}"),
        }

        assert_eq!(
            use_case.count().unwrap(),
            0,
            "a rejected entity type must not be registered"
        );
    }

    #[tokio::test]
    async fn test_execute_rejects_an_attribute_type_cedar_does_not_understand() {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case = RegisterEntityTypeUseCase::new(builder.clone());

        use_case
            .execute(RegisterEntityTypeCommand::with_attributes(
                "Iam::User".to_string(),
                vec![EntityAttribute::new(
                    "name".to_string(),
                    "NotACedarType".to_string(),
                )],
            ))
            .await
            .expect("the fragment itself is parseable");

        // The attribute type is neither a Cedar primitive nor a declared
        // entity type, so the schema built from the fragment must be
        // rejected: a registration that cannot be part of a valid schema
        // must not be reported as a usable one.
        let schema = {
            let mut locked = builder.lock().expect("builder lock");
            std::mem::replace(&mut *locked, EngineBuilder::new())
        }
        .build_schema();

        assert!(
            schema.is_err(),
            "a schema with an unknown attribute type must not be built"
        );
    }
}
