use serde::{Deserialize, Serialize};
use crate::shared::domain::{Hrn, Policy};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScpCommand {
    pub scp_id: String,
    pub scp_content: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteScpCommand {
    pub scp_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScpCommand {
    pub scp_id: String,
    pub scp_content: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetScpQuery {
    pub scp_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListScpsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDto {
    pub id: Hrn,
    pub content: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Policy> for ScpDto {
    fn from(policy: Policy) -> Self {
        ScpDto {
            id: policy.id,
            content: policy.content,
            description: policy.description,
            created_at: policy.created_at,
            updated_at: policy.updated_at,
        }
    }
}
