use serde::{Deserialize, Serialize};
use kernel::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAccountCommand {
    pub account_hrn: Hrn,
    pub source_ou_hrn: Hrn,
    pub target_ou_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
