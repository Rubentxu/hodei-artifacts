use serde::{Deserialize, Serialize};
use crate::domain::policy::PolicyId;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CreatePolicyCommand {
    #[validate(length(min = 1, message = "Policy name cannot be empty"))]
    pub name: String,
    pub description: Option<String>,
    #[validate(length(min = 1, message = "Policy content cannot be empty"))]
    pub content: String, // The Cedar policy text
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CreatePolicyResponse {
    pub id: PolicyId,
}