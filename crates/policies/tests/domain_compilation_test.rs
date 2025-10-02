//! Test to verify that the domain modules compile correctly

#[cfg(test)]
mod tests {
    use policies::domain::principals::{Group, User};
    use policies::domain::{EngineBuilder, HodeiEntityType};

    #[test]
    fn test_user_entity_type() {
        assert_eq!(User::entity_type_name(), "User");
    }

    #[test]
    fn test_group_entity_type() {
        assert_eq!(Group::entity_type_name(), "Group");
    }

    #[test]
    fn test_user_cedar_attributes_present() {
        let attrs = User::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "User should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_group_cedar_attributes_present() {
        let attrs = Group::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "Group should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_engine_builder() {
        let _builder = EngineBuilder::new();
        // Just testing that we can create an engine builder
        assert!(true);
    }
}
