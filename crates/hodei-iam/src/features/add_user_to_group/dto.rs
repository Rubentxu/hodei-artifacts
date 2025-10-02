/// Data Transfer Objects for add_user_to_group feature

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserToGroupCommand {
    pub user_hrn: String,
    pub group_hrn: String,
}

