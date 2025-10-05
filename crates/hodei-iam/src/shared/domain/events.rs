//! Domain events for the IAM bounded context
//!
//! These events represent state changes in the IAM domain that other
//! bounded contexts might be interested in.

use serde::{Deserialize, Serialize};
use kernel::Hrn;
use kernel::application::ports::event_bus::DomainEvent;

/// Event emitted when a new user is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreated {
    /// HRN of the created user
    pub user_hrn: Hrn,
    /// Username
    pub username: String,
    /// Email of the user
    pub email: String,
    /// Timestamp when the user was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserCreated {
    fn event_type(&self) -> &'static str {
        "iam.user.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a new group is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCreated {
    /// HRN of the created group
    pub group_hrn: Hrn,
    /// Group name
    pub name: String,
    /// Timestamp when the group was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupCreated {
    fn event_type(&self) -> &'static str {
        "iam.group.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a user is added to a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAddedToGroup {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the group
    pub group_hrn: Hrn,
    /// Timestamp when the user was added
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserAddedToGroup {
    fn event_type(&self) -> &'static str {
        "iam.user.added_to_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}
