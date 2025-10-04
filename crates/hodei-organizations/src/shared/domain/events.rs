//! Domain events for the Organizations bounded context
//!
//! These events represent state changes in the Organizations domain that other
//! bounded contexts might be interested in.

use policies::domain::Hrn;
use serde::{Deserialize, Serialize};
use shared::application::ports::event_bus::DomainEvent;

/// Event emitted when a new account is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCreated {
    /// HRN of the created account
    pub account_hrn: Hrn,
    /// Account name
    pub name: String,
    /// HRN of the parent OU (if any)
    pub parent_hrn: Option<Hrn>,
    /// Timestamp when the account was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountCreated {
    fn event_type(&self) -> &'static str {
        "organizations.account.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Event emitted when an account is moved between organizational units
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMoved {
    /// HRN of the account that was moved
    pub account_hrn: Hrn,
    /// HRN of the source OU (where it was before)
    pub from_ou_hrn: Option<Hrn>,
    /// HRN of the destination OU (where it is now)
    pub to_ou_hrn: Option<Hrn>,
    /// Timestamp when the account was moved
    pub moved_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountMoved {
    fn event_type(&self) -> &'static str {
        "organizations.account.moved"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Event emitted when an account is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDeleted {
    /// HRN of the deleted account
    pub account_hrn: Hrn,
    /// Timestamp when the account was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.account.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Type of target for SCP attachment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScpTargetType {
    Account,
    OrganizationalUnit,
    Root,
}

impl std::fmt::Display for ScpTargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScpTargetType::Account => write!(f, "account"),
            ScpTargetType::OrganizationalUnit => write!(f, "organizational_unit"),
            ScpTargetType::Root => write!(f, "root"),
        }
    }
}

/// Event emitted when a Service Control Policy (SCP) is attached to a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpAttached {
    /// HRN of the SCP that was attached
    pub scp_hrn: Hrn,
    /// HRN of the target (Account, OU, or Root)
    pub target_hrn: Hrn,
    /// Type of the target
    pub target_type: ScpTargetType,
    /// Timestamp when the SCP was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpAttached {
    fn event_type(&self) -> &'static str {
        "organizations.scp.attached"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.target_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is detached from a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDetached {
    /// HRN of the SCP that was detached
    pub scp_hrn: Hrn,
    /// HRN of the target (Account, OU, or Root)
    pub target_hrn: Hrn,
    /// Type of the target
    pub target_type: ScpTargetType,
    /// Timestamp when the SCP was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpDetached {
    fn event_type(&self) -> &'static str {
        "organizations.scp.detached"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.target_hrn.to_string())
    }
}

/// Event emitted when a new organizational unit is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnitCreated {
    /// HRN of the created OU
    pub ou_hrn: Hrn,
    /// OU name
    pub name: String,
    /// HRN of the parent OU (if any, None for root-level OUs)
    pub parent_hrn: Option<Hrn>,
    /// Timestamp when the OU was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for OrganizationalUnitCreated {
    fn event_type(&self) -> &'static str {
        "organizations.ou.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.ou_hrn.to_string())
    }
}

/// Event emitted when an organizational unit is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnitDeleted {
    /// HRN of the deleted OU
    pub ou_hrn: Hrn,
    /// Timestamp when the OU was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for OrganizationalUnitDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.ou.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.ou_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpCreated {
    /// HRN of the created SCP
    pub scp_hrn: Hrn,
    /// SCP name
    pub name: String,
    /// SCP description (optional)
    pub description: Option<String>,
    /// Timestamp when the SCP was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpCreated {
    fn event_type(&self) -> &'static str {
        "organizations.scp.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpUpdated {
    /// HRN of the updated SCP
    pub scp_hrn: Hrn,
    /// SCP name
    pub name: String,
    /// Timestamp when the SCP was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpUpdated {
    fn event_type(&self) -> &'static str {
        "organizations.scp.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDeleted {
    /// HRN of the deleted SCP
    pub scp_hrn: Hrn,
    /// Timestamp when the SCP was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.scp.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_created_event_type() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-123".to_string(),
        );

        let event = AccountCreated {
            account_hrn: hrn.clone(),
            name: "Test Account".to_string(),
            parent_hrn: None,
            created_at: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "organizations.account.created");
        assert_eq!(event.aggregate_id(), Some(hrn.to_string()));
    }

    #[test]
    fn test_scp_attached_event_type() {
        let scp_hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-123".to_string(),
        );

        let target_hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-456".to_string(),
        );

        let event = ScpAttached {
            scp_hrn,
            target_hrn: target_hrn.clone(),
            target_type: ScpTargetType::Account,
            attached_at: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "organizations.scp.attached");
        assert_eq!(event.aggregate_id(), Some(target_hrn.to_string()));
    }

    #[test]
    fn test_scp_target_type_display() {
        assert_eq!(ScpTargetType::Account.to_string(), "account");
        assert_eq!(
            ScpTargetType::OrganizationalUnit.to_string(),
            "organizational_unit"
        );
        assert_eq!(ScpTargetType::Root.to_string(), "root");
    }
}
