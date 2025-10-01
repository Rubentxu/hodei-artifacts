use std::fmt::Debug;
use serde_json::Value;
use shared::hrn::Hrn;
use chrono::{DateTime, Utc};
use async_trait::async_trait;

// Trait para abstraer el motor de autorización
#[async_trait::async_trait]
pub trait AuthorizationEnginePort: Debug + Send + Sync {
    async fn is_authorized(
        &self,
        principal: Hrn,
        action: Hrn,
        resource: Hrn,
        context: Option<Value>,
    ) -> crate::error::Result<AuthorizationResult>;
}

#[derive(Debug, Clone)]
pub struct AuthorizationResult {
    pub decision: String,
    pub reasons: Vec<String>,
}

// Trait para abstraer el almacenamiento de políticas
#[async_trait::async_trait]
pub trait PolicyStorePort: Debug + Send + Sync {
    async fn create_policy(&self, policy: Policy) -> crate::error::Result<String>;
    async fn list_policies(&self) -> crate::error::Result<Vec<Policy>>;
    async fn delete_policy(&self, policy_id: String) -> crate::error::Result<()>;
    async fn get_policy(&self, policy_id: String) -> crate::error::Result<Option<Policy>>;
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub policy_content: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
