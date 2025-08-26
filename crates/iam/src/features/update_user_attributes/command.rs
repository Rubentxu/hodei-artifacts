use serde::{Deserialize, Serialize};
use shared::UserId;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateUserAttributesCommand {
    pub user_id: UserId,
    pub attributes: serde_json::Value,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct UpdateUserAttributesResponse {
    pub user_id: UserId,
}