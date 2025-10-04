#[cfg(test)]
mod tests {
    use super::super::dto::AddUserToGroupCommand;
    use super::super::mocks::{
        MockAddUserToGroupUnitOfWork, MockGroupRepository, MockUserRepository,
    };
    use super::super::use_case::AddUserToGroupUseCase;
    use crate::shared::application::ports::UserRepository;
    use crate::shared::domain::{Group, User};
    use policies::shared::domain::hrn::Hrn;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_add_user_to_group_success() {
        // Arrange: Create test data
        let user_hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            "user1".to_string(),
        );
        let group_hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            "group1".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        let group = Group::new(group_hrn.clone(), "Developers".to_string());

        let user_repo = Arc::new(MockUserRepository::new().with_user(user));
        let group_repo = Arc::new(MockGroupRepository::new().with_group(group));

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_ok());

        // Verify user was updated with the group
        let updated_user = user_repo.find_by_hrn(&user_hrn).await.unwrap().unwrap();
        assert!(updated_user.groups().contains(&group_hrn));

        // Verify transaction was committed
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::Committed
        );
    }

    #[tokio::test]
    async fn test_add_user_to_group_user_not_found() {
        // Arrange: Create only group, no user
        let user_hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            "nonexistent".to_string(),
        );
        let group_hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            "group1".to_string(),
        );

        let group = Group::new(group_hrn.clone(), "Developers".to_string());

        let user_repo = Arc::new(MockUserRepository::new());
        let group_repo = Arc::new(MockGroupRepository::new().with_group(group));

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("User not found"));

        // Verify transaction was rolled back
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::RolledBack
        );
    }

    #[tokio::test]
    async fn test_add_user_to_group_group_not_found() {
        // Arrange: Create only user, no group
        let user_hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            "user1".to_string(),
        );
        let group_hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            "nonexistent".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );

        let user_repo = Arc::new(MockUserRepository::new().with_user(user));
        let group_repo = Arc::new(MockGroupRepository::new());

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Group not found"));

        // Verify transaction was rolled back
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::RolledBack
        );
    }

    #[tokio::test]
    async fn test_add_user_to_group_invalid_user_hrn() {
        // Arrange
        let user_repo = Arc::new(MockUserRepository::new());
        let group_repo = Arc::new(MockGroupRepository::new());

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: "invalid-hrn".to_string(),
            group_hrn: "hrn:hodei:iam:default:Group/group1".to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid user HRN"));
    }

    #[tokio::test]
    async fn test_add_user_to_group_invalid_group_hrn() {
        // Arrange
        let user_repo = Arc::new(MockUserRepository::new());
        let group_repo = Arc::new(MockGroupRepository::new());

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: Hrn::for_entity_type::<User>(
                "hodei".to_string(),
                "default".to_string(),
                "user1".to_string(),
            )
            .to_string(),
            group_hrn: "invalid-hrn".to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid group HRN"));
    }

    #[tokio::test]
    async fn test_add_user_to_group_idempotent() {
        // Arrange: User already in group
        let user_hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            "user1".to_string(),
        );
        let group_hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            "group1".to_string(),
        );

        let mut user = User::new(
            user_hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        user.add_to_group(group_hrn.clone()); // Already in group

        let group = Group::new(group_hrn.clone(), "Developers".to_string());

        let user_repo = Arc::new(MockUserRepository::new().with_user(user));
        let group_repo = Arc::new(MockGroupRepository::new().with_group(group));

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Act
        let result = use_case.execute(cmd).await;

        // Assert: Should succeed idempotently
        assert!(result.is_ok());

        // Verify user still has only one instance of the group
        let updated_user = user_repo.find_by_hrn(&user_hrn).await.unwrap().unwrap();
        assert_eq!(updated_user.groups().len(), 1);
        assert!(updated_user.groups().contains(&group_hrn));

        // Verify transaction was committed
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::Committed
        );
    }

    #[tokio::test]
    async fn test_add_user_to_group_transactional_integrity() {
        // Arrange: This test verifies that transaction lifecycle is properly managed
        let user_hrn = Hrn::for_entity_type::<User>(
            "hodei".to_string(),
            "default".to_string(),
            "user1".to_string(),
        );
        let group_hrn = Hrn::for_entity_type::<Group>(
            "hodei".to_string(),
            "default".to_string(),
            "group1".to_string(),
        );

        let user = User::new(
            user_hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        let group = Group::new(group_hrn.clone(), "Developers".to_string());

        let user_repo = Arc::new(MockUserRepository::new().with_user(user));
        let group_repo = Arc::new(MockGroupRepository::new().with_group(group));

        let uow = Arc::new(MockAddUserToGroupUnitOfWork::new(
            user_repo.clone(),
            group_repo.clone(),
        ));

        let use_case = AddUserToGroupUseCase::new(uow.clone());

        let cmd = AddUserToGroupCommand {
            user_hrn: user_hrn.to_string(),
            group_hrn: group_hrn.to_string(),
        };

        // Verify transaction starts as NotStarted
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::NotStarted
        );

        // Act
        let result = use_case.execute(cmd).await;

        // Assert
        assert!(result.is_ok());

        // Verify transaction ended as Committed
        assert_eq!(
            uow.transaction_state(),
            super::super::mocks::TransactionState::Committed
        );
    }
}
