use serde::Deserialize;
use cedar_policy::PolicyId;

#[derive(Debug, Deserialize)]
pub struct DeletePolicyCommand {
    pub id: PolicyId,
}
