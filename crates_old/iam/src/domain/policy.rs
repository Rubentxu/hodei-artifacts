use serde::{Deserialize, Serialize};
pub use cedar_policy::PolicyId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PolicyStatus {
    Draft,
    Active,
    Inactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Policy {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub version: u32,
    pub content: String, // The Cedar policy text
    pub status: PolicyStatus,
}
