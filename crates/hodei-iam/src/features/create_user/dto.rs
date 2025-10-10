//! Data Transfer Objects for create_user feature

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

/// Data Transfer Object for user persistence operations
///
/// This DTO is used to transfer user data to the persistence layer
/// without exposing the internal User domain entity.
#[derive(Debug, Clone)]
pub struct UserPersistenceDto {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub group_hrns: Vec<String>,
    pub tags: Vec<String>,
}

impl UserPersistenceDto {
    /// Create a new UserPersistenceDto
    pub fn new(hrn: impl Into<String>, name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            hrn: hrn.into(),
            name: name.into(),
            email: email.into(),
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }
}
