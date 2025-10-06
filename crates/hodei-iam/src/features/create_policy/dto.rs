use kernel::Hrn;
use policies::shared::domain::Policy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePolicyCommand {
    pub policy_id: String,
    pub policy_content: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyCommand {
    pub policy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicyCommand {
    pub policy_id: String,
    pub policy_content: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    pub policy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDto {
    pub id: Hrn,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Policy> for PolicyDto {
    fn from(policy: Policy) -> Self {
        PolicyDto {
            id: policy.id,
            content: policy.content,
            description: policy.description,
            created_at: policy.created_at,
            updated_at: policy.updated_at,
        }
    }
}

/// Alias for PolicyDto to match naming convention used in exports
pub type PolicyView = PolicyDto;
