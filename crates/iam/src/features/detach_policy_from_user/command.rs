use serde::Deserialize;
use shared::UserId;
use cedar_policy::PolicyId;

#[derive(Debug, Deserialize)]
pub struct DetachPolicyFromUserCommand {
    pub user_id: UserId,
    pub policy_id: PolicyId,
}
