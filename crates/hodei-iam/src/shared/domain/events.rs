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

/// Event emitted when a user is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdated {
    /// HRN of the updated user
    pub user_hrn: Hrn,
    /// Username
    pub username: String,
    /// Timestamp when the user was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserUpdated {
    fn event_type(&self) -> &'static str {
        "iam.user.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a user is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeleted {
    /// HRN of the deleted user
    pub user_hrn: Hrn,
    /// Timestamp when the user was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserDeleted {
    fn event_type(&self) -> &'static str {
        "iam.user.deleted"
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

/// Event emitted when a group is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupUpdated {
    /// HRN of the updated group
    pub group_hrn: Hrn,
    /// Group name
    pub name: String,
    /// Timestamp when the group was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupUpdated {
    fn event_type(&self) -> &'static str {
        "iam.group.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a group is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDeleted {
    /// HRN of the deleted group
    pub group_hrn: Hrn,
    /// Timestamp when the group was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupDeleted {
    fn event_type(&self) -> &'static str {
        "iam.group.deleted"
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

/// Event emitted when a user is removed from a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRemovedFromGroup {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the group
    pub group_hrn: Hrn,
    /// Timestamp when the user was removed
    pub removed_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserRemovedFromGroup {
    fn event_type(&self) -> &'static str {
        "iam.user.removed_from_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a policy is attached to a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAttachedToUser {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyAttachedToUser {
    fn event_type(&self) -> &'static str {
        "iam.policy.attached_to_user"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a policy is detached from a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDetachedFromUser {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyDetachedFromUser {
    fn event_type(&self) -> &'static str {
        "iam.policy.detached_from_user"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a policy is attached to a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAttachedToGroup {
    /// HRN of the group
    pub group_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyAttachedToGroup {
    fn event_type(&self) -> &'static str {
        "iam.policy.attached_to_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a policy is detached from a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDetachedFromGroup {
    /// HRN of the group
    pub group_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyDetachedFromGroup {
    fn event_type(&self) -> &'static str {
        "iam.policy.detached_from_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}
