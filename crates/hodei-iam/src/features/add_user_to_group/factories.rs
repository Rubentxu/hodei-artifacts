//! Factory for creating the AddUserToGroup use case
//!
//! This module follows the trait objects pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn UseCasePort> for maximum flexibility
//! - Easy testing with mock implementations

use std::sync::Arc;
use tracing::info;

use crate::features::add_user_to_group::ports::{
    AddUserToGroupUseCasePort, GroupFinder, UserFinder, UserGroupPersister,
};
use crate::features::add_user_to_group::use_case::AddUserToGroupUseCase;

/// Create the AddUserToGroup use case with injected dependencies
///
/// This factory receives trait objects and returns a trait object,
/// making it simple to use from the Composition Root and easy to test.
///
/// # Arguments
///
/// * `user_finder` - Port for finding users by HRN
/// * `group_finder` - Port for finding groups by HRN
/// * `user_persister` - Port for persisting user updates
///
/// # Returns
///
/// Arc<dyn AddUserToGroupUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let user_finder = Arc::new(SurrealUserAdapter::new(db));
/// let group_finder = Arc::new(SurrealGroupAdapter::new(db));
/// let user_persister = Arc::new(SurrealUserAdapter::new(db));
///
/// let add_user_to_group = create_add_user_to_group_use_case(
///     user_finder,
///     group_finder,
///     user_persister,
/// );
/// ```
pub fn create_add_user_to_group_use_case(
    user_finder: Arc<dyn UserFinder>,
    group_finder: Arc<dyn GroupFinder>,
    user_persister: Arc<dyn UserGroupPersister>,
) -> Arc<dyn AddUserToGroupUseCasePort> {
    info!("Creating AddUserToGroup use case");
    Arc::new(AddUserToGroupUseCase::new(
        user_finder,
        group_finder,
        user_persister,
    ))
}

/// Alternative factory that accepts owned dependencies
///
/// This is useful when you have dependencies that are not yet wrapped in Arc
/// and you want the factory to handle the Arc wrapping.
///
/// # Arguments
///
/// * `user_finder` - Port for finding users by HRN
/// * `group_finder` - Port for finding groups by HRN
/// * `user_persister` - Port for persisting user updates
///
/// # Returns
///
/// Arc<dyn AddUserToGroupUseCasePort> - The use case as a trait object
pub fn create_add_user_to_group_use_case_from_owned<UF, GF, UP>(
    user_finder: UF,
    group_finder: GF,
    user_persister: UP,
) -> Arc<dyn AddUserToGroupUseCasePort>
where
    UF: UserFinder + 'static,
    GF: GroupFinder + 'static,
    UP: UserGroupPersister + 'static,
{
    info!("Creating AddUserToGroup use case from owned dependencies");
    Arc::new(AddUserToGroupUseCase::new(
        Arc::new(user_finder),
        Arc::new(group_finder),
        Arc::new(user_persister),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::add_user_to_group::dto::AddUserToGroupCommand;
    use crate::features::add_user_to_group::mocks::{
        MockGroupFinder, MockUserFinder, MockUserGroupPersister,
    };

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let user_finder: Arc<dyn UserFinder> = Arc::new(MockUserFinder::new());
        let group_finder: Arc<dyn GroupFinder> = Arc::new(MockGroupFinder::new());
        let user_persister: Arc<dyn UserGroupPersister> = Arc::new(MockUserGroupPersister::new());

        let use_case = create_add_user_to_group_use_case(
            user_finder,
            group_finder,
            user_persister,
        );

        let command = AddUserToGroupCommand {
            user_hrn: "hrn:hodei:iam:user:test-user".to_string(),
            group_hrn: "hrn:hodei:iam:group:test-group".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_from_owned_works() {
        let user_finder = MockUserFinder::new();
        let group_finder = MockGroupFinder::new();
        let user_persister = MockUserGroupPersister::new();

        let use_case = create_add_user_to_group_use_case_from_owned(
            user_finder,
            group_finder,
            user_persister,
        );

        let command = AddUserToGroupCommand {
            user_hrn: "hrn:hodei:iam:user:test-user".to_string(),
            group_hrn: "hrn:hodei:iam:group:test-group".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
