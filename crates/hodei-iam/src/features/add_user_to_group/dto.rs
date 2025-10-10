//! Data Transfer Objects for add_user_to_group feature

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserToGroupCommand {
    pub user_hrn: String,
    pub group_hrn: String,
}

/// Data Transfer Object for user lookup operations
///
/// This DTO is used to transfer user data from the persistence layer
/// without exposing the internal User domain entity.
#[derive(Debug, Clone)]
pub struct UserLookupDto {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub group_hrns: Vec<String>,
    pub tags: Vec<String>,
}

impl UserLookupDto {
    /// Create a new UserLookupDto
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

/// Data Transfer Object for group lookup operations
///
/// This DTO is used to transfer group data from the persistence layer
/// without exposing the internal Group domain entity.
#[derive(Debug, Clone)]
pub struct GroupLookupDto {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

impl GroupLookupDto {
    /// Create a new GroupLookupDto
    pub fn new(hrn: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            hrn: hrn.into(),
            name: name.into(),
            tags: Vec::new(),
        }
    }
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
