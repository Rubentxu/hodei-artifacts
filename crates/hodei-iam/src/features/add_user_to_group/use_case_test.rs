#[cfg(test)]
mod tests {
    use super::super::dto::{
        AddUserToGroupCommand, GroupLookupDto, UserLookupDto, UserPersistenceDto,
    };
    use super::super::error::AddUserToGroupError;
    use super::super::ports::{GroupFinder, UserFinder, UserGroupPersister};
    use super::super::use_case::AddUserToGroupUseCase;
    use crate::internal::domain::{Group, User};
    use kernel::Hrn;
    use std::sync::Arc;

    // Mock implementation of UserFinder
    struct MockUserFinder {
        user: Option<UserLookupDto>,
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl UserFinder for MockUserFinder {
        async fn find_user_by_hrn(
            &self,
            _hrn: &Hrn,
        ) -> Result<Option<UserLookupDto>, AddUserToGroupError> {
            if self.should_fail {
                Err(AddUserToGroupError::PersistenceError(
                    "Failed to find user".to_string(),
                ))
            } else {
                Ok(self.user.clone())
            }
        }
    }

    // Mock implementation of GroupFinder
    struct MockGroupFinder {
        group: Option<GroupLookupDto>,
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl GroupFinder for MockGroupFinder {
        async fn find_group_by_hrn(
            &self,
            _hrn: &Hrn,
        ) -> Result<Option<GroupLookupDto>, AddUserToGroupError> {
            if self.should_fail {
                Err(AddUserToGroupError::PersistenceError(
                    "Failed to find group".to_string(),
                ))
            } else {
                Ok(self.group.clone())
            }
        }
    }

    // Mock implementation of UserGroupPersister
    struct MockUserGroupPersister {
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl UserGroupPersister for MockUserGroupPersister {
        async fn save_user(
            &self,
            _user_dto: &UserPersistenceDto,
        ) -> Result<(), AddUserToGroupError> {
            if self.should_fail {
                Err(AddUserToGroupError::PersistenceError(
                    "Failed to save user".to_string(),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_add_user_to_group_success() {
        // Arrange
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );

        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Test User".to_string(),
            "test@example.com".to_string(),
        );
        let group = Group::new(group_hrn.clone(), "Test Group".to_string(), None);

        let user_dto = UserLookupDto {
            hrn: user_hrn.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            group_hrns: user.group_hrns.iter().map(|hrn| hrn.to_string()).collect(),
            tags: user.tags.clone(),
        };
        let group_dto = GroupLookupDto {
            hrn: group_hrn.to_string(),
            name: group.name.clone(),
            tags: group.tags.clone(),
        };

        let user_finder = Arc::new(MockUserFinder {
            user: Some(user_dto),
            should_fail: false,
        });
        let group_finder = Arc::new(MockGroupFinder {
            group: Some(group_dto),
            should_fail: false,
        });
        let user_persister = Arc::new(MockUserGroupPersister { should_fail: false });

        let use_case = AddUserToGroupUseCase::new(user_finder, group_finder, user_persister);

        let command = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_user_to_group_user_not_found() {
        // Arrange
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );

        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
        );

        let group = Group::new(group_hrn.clone(), "Test Group".to_string(), None);

        let group_dto = GroupLookupDto {
            hrn: group_hrn.to_string(),
            name: group.name.clone(),
            tags: group.tags.clone(),
        };

        let user_finder = Arc::new(MockUserFinder {
            user: None,
            should_fail: false,
        });
        let group_finder = Arc::new(MockGroupFinder {
            group: Some(group_dto),
            should_fail: false,
        });
        let user_persister = Arc::new(MockUserGroupPersister { should_fail: false });

        let use_case = AddUserToGroupUseCase::new(user_finder, group_finder, user_persister);

        let command = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            AddUserToGroupError::UserNotFound(_) => (),
            _ => panic!("Expected UserNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_add_user_to_group_group_not_found() {
        // Arrange
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );

        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Test User".to_string(),
            "test@example.com".to_string(),
        );

        let user_dto = UserLookupDto {
            hrn: user_hrn.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            group_hrns: user.group_hrns.iter().map(|hrn| hrn.to_string()).collect(),
            tags: user.tags.clone(),
        };

        let user_finder = Arc::new(MockUserFinder {
            user: Some(user_dto),
            should_fail: false,
        });
        let group_finder = Arc::new(MockGroupFinder {
            group: None,
            should_fail: false,
        });
        let user_persister = Arc::new(MockUserGroupPersister { should_fail: false });

        let use_case = AddUserToGroupUseCase::new(user_finder, group_finder, user_persister);

        let command = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            AddUserToGroupError::GroupNotFound(_) => (),
            _ => panic!("Expected GroupNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_add_user_to_group_persistence_error() {
        // Arrange
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "test-user".to_string(),
        );

        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "test-group".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Test User".to_string(),
            "test@example.com".to_string(),
        );
        let group = Group::new(group_hrn.clone(), "Test Group".to_string(), None);

        let user_dto = UserLookupDto {
            hrn: user_hrn.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            group_hrns: user.group_hrns.iter().map(|hrn| hrn.to_string()).collect(),
            tags: user.tags.clone(),
        };
        let group_dto = GroupLookupDto {
            hrn: group_hrn.to_string(),
            name: group.name.clone(),
            tags: group.tags.clone(),
        };

        let user_finder = Arc::new(MockUserFinder {
            user: Some(user_dto),
            should_fail: false,
        });
        let group_finder = Arc::new(MockGroupFinder {
            group: Some(group_dto),
            should_fail: false,
        });
        let user_persister = Arc::new(MockUserGroupPersister { should_fail: true });

        let use_case = AddUserToGroupUseCase::new(user_finder, group_finder, user_persister);

        let command = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(command).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            AddUserToGroupError::PersistenceError(_) => (),
            _ => panic!("Expected PersistenceError"),
        }
    }
}
