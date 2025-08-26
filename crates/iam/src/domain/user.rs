use cedar_policy::PolicyId;
use serde::{Deserialize, Serialize};
use shared::UserId;

// Using a newtype pattern for type safety
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct GroupId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UserStatus {
    Active,
    Inactive,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub status: UserStatus,
    // Atributos flexibles para el motor de políticas ABAC (Cedar)
    pub attributes: serde_json::Value,
    pub groups: Vec<GroupId>,
    pub policies: Vec<PolicyId>,
}
