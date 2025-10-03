/// Data Transfer Objects for create_group feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupCommand {
    pub group_name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

