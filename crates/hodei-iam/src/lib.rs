/// hodei-iam: Default IAM entities for the policies engine
/// 
/// This crate provides a standard set of Identity and Access Management entities
/// that can be used with the policies engine. It follows the same Vertical Slice
/// Architecture (VSA) with hexagonal architecture as the policies crate.
/// 
/// # Structure
/// - `shared/domain`: Core IAM entities (User, Group, ServiceAccount, Namespace) and actions
/// - `shared/application`: Ports (repository traits) and DI configurator
/// - `shared/infrastructure`: Infrastructure adapters (in-memory repositories for testing)
/// - `features`: IAM-specific features/use cases (create_user, create_group, add_user_to_group)
///
/// # Example
/// ```no_run
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// use policies::shared::application::di_helpers;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// // Build an engine with default IAM entities
/// let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
/// # Ok(())
/// # }
/// ```

pub mod shared;
pub mod features;

// Re-export commonly used items for convenience
pub use shared::domain::{User, Group, ServiceAccount, Namespace, CreateUserAction, CreateGroupAction};
pub use shared::application::configure_default_iam_entities;

// Re-export features for easy access
pub use features::{
    create_group::CreateGroupUseCase,
    create_user::CreateUserUseCase,
    add_user_to_group::AddUserToGroupUseCase,
};

#[cfg(test)]
mod tests {
    use super::*;
    use policies::shared::domain::ports::{Principal, Resource, Action};
    use policies::shared::domain::hrn::Hrn;

    fn sample_group(id: &str) -> Group {
        Group {
            hrn: Hrn::new(
                "aws".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                id.into(),
            ),
            name: format!("group-{}", id),
            tags: vec!["team".into()],
            attached_policy_hrns: vec![],
        }
    }

    #[test]
    fn group_attributes_contains_expected_keys() {
        use policies::shared::domain::ports::HodeiEntity;
        let g = sample_group("dev");
        let attrs = g.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("tags"));
    }

    #[test]
    fn user_parents_produce_entityuids() {
        let groups = vec![
            Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                "dev".into(),
            ),
            Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                "ops".into(),
            ),
        ];
        let user = User {
            hrn: Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "User".into(),
                "alice".into(),
            ),
            name: "Alice".into(),
            group_hrns: groups,
            email: "alice@example.com".into(),
            tags: vec!["admin".into()],
        };
        use policies::shared::domain::ports::HodeiEntity;
        let parents = user.parents();
        assert_eq!(parents.len(), 2);
        let s0 = format!("{}", parents[0]);
        assert!(s0.contains("Group"));
    }

    #[test]
    fn user_attributes_contains_expected() {
        let user = User {
            hrn: Hrn::new(
                "default".into(),
                "hodei".into(),
                "123".into(),
                "User".into(),
                "alice".into(),
            ),
            name: "Alice".into(),
            group_hrns: vec![],
            email: "alice@example.com".into(),
            tags: vec!["owner".into()],
        };
        use policies::shared::domain::ports::HodeiEntity;
        let attrs = user.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("email"));
        assert!(attrs.contains_key("tags"));
    }
    
    #[test]
    fn user_implements_principal_trait() {
        fn assert_is_principal<T: Principal>() {}
        assert_is_principal::<User>();
    }
    
    #[test]
    fn user_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<User>();
    }
    
    #[test]
    fn group_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<Group>();
    }
    
    #[test]
    fn service_account_implements_principal_trait() {
        fn assert_is_principal<T: Principal>() {}
        assert_is_principal::<ServiceAccount>();
    }
    
    #[test]
    fn service_account_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<ServiceAccount>();
    }
    
    #[test]
    fn namespace_implements_resource_trait() {
        fn assert_is_resource<T: Resource>() {}
        assert_is_resource::<Namespace>();
    }
    
    #[test]
    fn create_user_action_implements_action_trait() {
        assert_eq!(CreateUserAction::name(), "create_user");
        let (principal, resource) = CreateUserAction::applies_to();
        assert_eq!(principal.to_string(), "User");
        assert_eq!(resource.to_string(), "User");
    }
    
    #[test]
    fn create_group_action_implements_action_trait() {
        assert_eq!(CreateGroupAction::name(), "create_group");
        let (principal, resource) = CreateGroupAction::applies_to();
        assert_eq!(principal.to_string(), "User");
        assert_eq!(resource.to_string(), "Group");
    }
}
