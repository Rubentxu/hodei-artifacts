use serde::{Deserialize, Serialize};
use kernel::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountCommand {
    pub name: String,
    pub parent_hrn: Option<Hrn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Option<Hrn>,
}
