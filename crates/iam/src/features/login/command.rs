use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginCommand {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct LoginResponse {
    pub token: String,
}
