//! DTOs for Get Policy feature

use kernel::Hrn;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;
use serde::{Deserialize, Serialize};

/// Query to fetch an IAM policy by its HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    /// HRN of the policy to fetch
    pub policy_hrn: Hrn,
}

impl ActionTrait for GetPolicyQuery {
    fn name() -> &'static str {
        "GetPolicy"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Policy".to_string()
    }
}

/// View of an IAM policy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyView {
    /// Unique HRN of the policy
    pub hrn: Hrn,

    /// Name of the policy
    pub name: String,

    /// Content of the policy in Cedar format
    pub content: String,

    /// Optional description of the policy
    pub description: Option<String>,

    /// The principals the policy is attached to
    ///
    /// The attachment is what makes the policy *effective* for a
    /// principal, so it is reported so that the consumer can confirm
    /// what was persisted.
    pub attached_principals: Vec<String>,

    /// When the policy was created, as it was persisted
    ///
    /// It is `None` for a row written before timestamps existed:
    /// there is nothing to return, and returning the process clock would be
    /// inventing data.
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,

    /// When the policy was last updated, as it was
    /// persisted
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
