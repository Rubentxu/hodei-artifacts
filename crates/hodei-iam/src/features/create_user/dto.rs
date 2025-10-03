/// Data Transfer Objects for create_user feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserCommand {
    pub name: String,
    pub email: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}

