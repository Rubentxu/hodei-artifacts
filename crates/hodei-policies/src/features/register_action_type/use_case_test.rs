#[cfg(test)]
mod tests {
    use super::super::dto::RegisterActionTypeCommand;
    use super::super::error::RegisterActionTypeError;
    use super::super::use_case::RegisterActionTypeUseCase;
    use crate::internal::engine::builder::EngineBuilder;
    use kernel::{ActionTrait, ServiceName};
    use std::sync::{Arc, Mutex};

    // Mock action types for testing

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

    struct CreateUserAction;

    impl ActionTrait for CreateUserAction {
        fn name() -> &'static str {
            "CreateUser"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Iam::Account".to_string()
        }
    }

    fn create_use_case() -> RegisterActionTypeUseCase {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        RegisterActionTypeUseCase::new(builder)
    }

    #[test]
    fn test_register_single_action_type() {
        let use_case = create_use_case();

        let result = use_case.register::<ReadAction>();

        assert!(
            result.is_ok(),
            "Failed to register action type: {:?}",
            result
        );
        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[test]
    fn test_register_multiple_action_types() {
        let use_case = create_use_case();

        let result1 = use_case.register::<ReadAction>();
        let result2 = use_case.register::<WriteAction>();
        let result3 = use_case.register::<DeleteAction>();

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
        assert_eq!(use_case.count().unwrap(), 3);
    }

    #[test]
    fn test_register_action_types_from_different_services() {
        let use_case = create_use_case();

        let result1 = use_case.register::<ReadAction>();
        let result2 = use_case.register::<CreateUserAction>();

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(use_case.count().unwrap(), 2);
    }

    #[test]
    fn test_clear_registered_action_types() {
        let use_case = create_use_case();

        use_case.register::<ReadAction>().unwrap();
        use_case.register::<WriteAction>().unwrap();

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

        use_case.register::<ReadAction>().unwrap();
        assert_eq!(use_case.count().unwrap(), 1);

        use_case.clear().unwrap();
        assert_eq!(use_case.count().unwrap(), 0);

        use_case.register::<WriteAction>().unwrap();
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
    fn test_action_name_format() {
        // Verify that the action names are correctly formatted
        assert_eq!(ReadAction::action_name(), "Storage::Action::\"Read\"");
        assert_eq!(WriteAction::action_name(), "Storage::Action::\"Write\"");
        assert_eq!(DeleteAction::action_name(), "Storage::Action::\"Delete\"");
        assert_eq!(
            CreateUserAction::action_name(),
            "Iam::Action::\"CreateUser\""
        );
    }

    #[test]
    fn test_action_applies_to() {
        assert_eq!(ReadAction::applies_to_principal(), "Iam::User");
        assert_eq!(ReadAction::applies_to_resource(), "Storage::Document");

        assert_eq!(CreateUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateUserAction::applies_to_resource(), "Iam::Account");
    }

    #[test]
    fn test_shared_builder_across_multiple_use_cases() {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case1 = RegisterActionTypeUseCase::new(builder.clone());
        let use_case2 = RegisterActionTypeUseCase::new(builder.clone());

        use_case1.register::<ReadAction>().unwrap();
        use_case2.register::<WriteAction>().unwrap();

        // Both use cases should see the same builder state
        assert_eq!(use_case1.count().unwrap(), 2);
        assert_eq!(use_case2.count().unwrap(), 2);
    }

    #[test]
    fn test_action_service_names() {
        assert_eq!(ReadAction::service_name().as_str(), "storage");
        assert_eq!(WriteAction::service_name().as_str(), "storage");
        assert_eq!(DeleteAction::service_name().as_str(), "storage");
        assert_eq!(CreateUserAction::service_name().as_str(), "iam");
    }

    #[test]
    fn test_register_multiple_actions_then_clear_all() {
        let use_case = create_use_case();

        use_case.register::<ReadAction>().unwrap();
        use_case.register::<WriteAction>().unwrap();
        use_case.register::<DeleteAction>().unwrap();
        use_case.register::<CreateUserAction>().unwrap();

        assert_eq!(use_case.count().unwrap(), 4);

        use_case.clear().unwrap();
        assert_eq!(use_case.count().unwrap(), 0);
    }

    // -----------------------------------------------------------------
    // The command-based path
    //
    // `execute` is the entry point the ports expose, so it is the one that
    // has to produce a real Cedar fragment. A command that registers nothing
    // but reports success would leave the schema empty while every caller
    // believes the action was declared.
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_execute_registers_the_action_type_of_the_command() {
        let use_case = create_use_case();

        use_case
            .execute(RegisterActionTypeCommand::new(
                "CreateUser".to_string(),
                "Iam::User".to_string(),
                "Iam::User".to_string(),
            ))
            .await
            .expect("a well-formed action must be registrable");

        assert_eq!(use_case.count().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_execute_registers_a_schema_that_cedar_accepts() {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case = RegisterActionTypeUseCase::new(builder.clone());

        // The `appliesTo` clause of an action references entity types, so
        // those have to be declared in the same schema: Cedar rejects a
        // schema whose action refers to an undeclared entity type.
        let entity_registrar =
            crate::features::register_entity_type::use_case::RegisterEntityTypeUseCase::new(
                builder.clone(),
            );
        entity_registrar
            .execute(
                crate::features::register_entity_type::dto::RegisterEntityTypeCommand::new(
                    "Iam::User".to_string(),
                ),
            )
            .await
            .expect("the referenced entity type must be registrable");

        use_case
            .execute(RegisterActionTypeCommand::new(
                "CreateUser".to_string(),
                "Iam::User".to_string(),
                "Iam::User".to_string(),
            ))
            .await
            .expect("the action type must be registrable");

        // The fragment has to survive Cedar's own schema construction.
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
            schema.actions().count(),
            1,
            "the built schema must declare exactly the registered action"
        );
    }

    #[tokio::test]
    async fn test_execute_is_idempotent_for_the_same_action_type() {
        let use_case = create_use_case();

        let command = RegisterActionTypeCommand::new(
            "CreateUser".to_string(),
            "Iam::User".to_string(),
            "Iam::User".to_string(),
        );

        use_case.execute(command.clone()).await.unwrap();
        use_case.execute(command.clone()).await.unwrap();

        assert_eq!(
            use_case.count().unwrap(),
            1,
            "registering the same action type twice must keep one fragment"
        );
    }

    #[tokio::test]
    async fn test_execute_rejects_an_action_without_a_name() {
        let use_case = create_use_case();

        let result = use_case
            .execute(RegisterActionTypeCommand::new(
                "   ".to_string(),
                "Iam::User".to_string(),
                "Iam::User".to_string(),
            ))
            .await;

        match result {
            Err(RegisterActionTypeError::InvalidActionType(_)) => {}
            other => panic!("expected InvalidActionType, got: {other:?}"),
        }

        assert_eq!(
            use_case.count().unwrap(),
            0,
            "a rejected action type must not be registered"
        );
    }

    #[tokio::test]
    async fn test_execute_rejects_an_action_without_a_principal_type() {
        let use_case = create_use_case();

        let result = use_case
            .execute(RegisterActionTypeCommand::new(
                "CreateUser".to_string(),
                String::new(),
                "Iam::User".to_string(),
            ))
            .await;

        match result {
            Err(RegisterActionTypeError::InvalidActionType(_)) => {}
            other => panic!("expected InvalidActionType, got: {other:?}"),
        }

        assert_eq!(use_case.count().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_execute_rejects_an_action_without_a_resource_type() {
        let use_case = create_use_case();

        let result = use_case
            .execute(RegisterActionTypeCommand::new(
                "CreateUser".to_string(),
                "Iam::User".to_string(),
                String::new(),
            ))
            .await;

        match result {
            Err(RegisterActionTypeError::InvalidActionType(_)) => {}
            other => panic!("expected InvalidActionType, got: {other:?}"),
        }

        assert_eq!(use_case.count().unwrap(), 0);
    }
}
