use std::sync::Arc;
use crate::application::ports::{UserRepository, PolicyRepository, PolicyValidator};
use crate::error::IamError;
use crate::features::create_user::{CreateUserCommand, handle_create_user};
use crate::features::update_user_attributes::{UpdateUserAttributesCommand, handle_update_user_attributes};
use crate::features::create_policy::{CreatePolicyCommand, handle_create_policy};
use crate::features::get_user::{GetUserQuery, handle_get_user};
use crate::features::list_users::{ListUsersQuery, handle_list_users};
use crate::features::delete_user::{DeleteUserCommand, handle_delete_user};
use crate::features::login::{LoginCommand, LoginResponse, handle_login};
use crate::features::get_policy::{GetPolicyQuery, handle_get_policy};
use crate::features::list_policies::{ListPoliciesQuery, handle_list_policies};
use crate::features::delete_policy::{DeletePolicyCommand, handle_delete_policy};
use crate::features::attach_policy_to_user::{AttachPolicyToUserCommand, handle_attach_policy_to_user};
use crate::features::detach_policy_from_user::{DetachPolicyFromUserCommand, handle_detach_policy_from_user};
use crate::domain::Policy;
use shared::UserId;
use cedar_policy::PolicyId;
use crate::domain::user::User;

pub struct IamApi {
    user_repository: Arc<dyn UserRepository>,
    policy_repository: Arc<dyn PolicyRepository>,
    policy_validator: Arc<dyn PolicyValidator>,
}

impl IamApi {
    pub fn new(
        user_repository: Arc<dyn UserRepository>,
        policy_repository: Arc<dyn PolicyRepository>,
        policy_validator: Arc<dyn PolicyValidator>,
    ) -> Self {
        Self { user_repository, policy_repository, policy_validator }
    }

    pub async fn create_user(&self, command: CreateUserCommand) -> Result<UserId, IamError> {
        handle_create_user(
            self.user_repository.as_ref(),
            command,
        ).await
    }

    pub async fn update_user_attributes(&self, command: UpdateUserAttributesCommand) -> Result<(), IamError> {
        handle_update_user_attributes(
            self.user_repository.as_ref(),
            command,
        ).await
    }

    pub async fn create_policy(&self, command: CreatePolicyCommand) -> Result<PolicyId, IamError> {
        handle_create_policy(
            self.policy_repository.as_ref(),
            self.policy_validator.as_ref(),
            command,
        ).await
    }

    pub async fn get_user(&self, query: GetUserQuery) -> Result<User, IamError> {
        handle_get_user(
            self.user_repository.as_ref(),
            query,
        ).await
    }

    pub async fn list_users(&self, query: ListUsersQuery) -> Result<Vec<User>, IamError> {
        handle_list_users(
            self.user_repository.as_ref(),
            query,
        ).await
    }

    pub async fn delete_user(&self, command: DeleteUserCommand) -> Result<(), IamError> {
        handle_delete_user(
            self.user_repository.as_ref(),
            command,
        ).await
    }

    pub async fn login(&self, command: LoginCommand) -> Result<LoginResponse, IamError> {
        handle_login(
            self.user_repository.as_ref(),
            command,
        ).await
    }

    pub async fn get_policy(&self, query: GetPolicyQuery) -> Result<Policy, IamError> {
        handle_get_policy(
            self.policy_repository.as_ref(),
            query,
        ).await
    }

    pub async fn list_policies(&self, query: ListPoliciesQuery) -> Result<Vec<Policy>, IamError> {
        handle_list_policies(
            self.policy_repository.as_ref(),
            query,
        ).await
    }

    pub async fn delete_policy(&self, command: DeletePolicyCommand) -> Result<(), IamError> {
        handle_delete_policy(
            self.policy_repository.as_ref(),
            command,
        ).await
    }

    pub async fn attach_policy_to_user(&self, command: AttachPolicyToUserCommand) -> Result<(), IamError> {
        handle_attach_policy_to_user(
            self.user_repository.as_ref(),
            self.policy_repository.as_ref(),
            command,
        ).await
    }

    pub async fn detach_policy_from_user(&self, command: DetachPolicyFromUserCommand) -> Result<(), IamError> {
        handle_detach_policy_from_user(
            self.user_repository.as_ref(),
            command,
        ).await
    }
}
