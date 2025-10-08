use std::sync::Arc;
use super::ports::{UserFinder, GroupFinder, UserGroupPersister};
use super::use_case::AddUserToGroupUseCase;

/// Factory for creating AddUserToGroupUseCase instances
///
/// This factory encapsulates the dependency injection logic for the
/// AddUserToGroupUseCase, making it easier to construct instances with
/// different implementations of the ports.
pub struct AddUserToGroupUseCaseFactory;

impl AddUserToGroupUseCaseFactory {
    /// Build an AddUserToGroupUseCase instance
    ///
    /// # Arguments
    /// * `user_finder` - Implementation of UserFinder for user lookup
    /// * `group_finder` - Implementation of GroupFinder for group lookup
    /// * `user_persister` - Implementation of UserGroupPersister for user persistence
    ///
    /// # Returns
    /// * A new AddUserToGroupUseCase instance
    pub fn build<UF, GF, UP>(
        user_finder: Arc<UF>,
        group_finder: Arc<GF>,
        user_persister: Arc<UP>,
    ) -> AddUserToGroupUseCase<UF, GF, UP>
    where
        UF: UserFinder,
        GF: GroupFinder,
        UP: UserGroupPersister,
    {
        AddUserToGroupUseCase::new(user_finder, group_finder, user_persister)
    }
}