//! Data Transfer Objects for create_group feature

use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupCommand {
    pub group_name: String,
    pub tags: Vec<String>,
}

impl ActionTrait for CreateGroupCommand {
    fn name() -> &'static str {
        "CreateGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

/// Data Transfer Object for group persistence operations
///
/// This DTO is used to transfer group data to the persistence layer
/// without exposing the internal Group domain entity.
#[derive(Debug, Clone)]
pub struct GroupPersistenceDto {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}

impl GroupPersistenceDto {
    /// Create a new GroupPersistenceDto
    pub fn new(hrn: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            hrn: hrn.into(),
            name: name.into(),
            tags: Vec::new(),
        }
    }
}
