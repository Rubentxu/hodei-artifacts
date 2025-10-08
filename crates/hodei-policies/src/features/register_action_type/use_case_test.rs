#[cfg(test)]
mod tests {
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
}
