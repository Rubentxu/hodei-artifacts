pub mod error;
pub mod domain;
pub mod application;
pub mod infrastructure;
pub mod features;

pub use domain::user::User;
pub use features::create_user::CreateUserCommand;
pub use features::update_user_attributes::UpdateUserAttributesCommand;
pub use features::get_user::GetUserQuery;
pub use features::list_users::ListUsersQuery;
pub use features::delete_user::DeleteUserCommand;
pub use features::login::{LoginCommand, LoginResponse};
pub use features::get_policy::GetPolicyQuery;
pub use features::list_policies::ListPoliciesQuery;
pub use features::delete_policy::DeletePolicyCommand;

pub mod mocks;
pub use features::attach_policy_to_user::AttachPolicyToUserCommand;
pub use features::detach_policy_from_user::DetachPolicyFromUserCommand;