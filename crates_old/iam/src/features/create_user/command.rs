use serde::{Deserialize, Serialize};
use shared::UserId;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserCommand {
    pub username: String,
    #[validate(email)]
    pub email: String,
    pub password: String,
    pub attributes: serde_json::Value,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CreateUserResponse {
    pub id: UserId,
}
