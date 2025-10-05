use crate::shared::domain::Policy;
use crate::shared::domain::ports::PolicyStorage;
use crate::features::create_scp::error::{CreateScpError, DeleteScpError, UpdateScpError, GetScpError, ListScpsError};
use crate::features::create_scp::dto::{CreateScpCommand, DeleteScpCommand, UpdateScpCommand, GetScpQuery, ListScpsQuery};
use async_trait::async_trait;

#[async_trait]
pub trait ScpPersister: PolicyStorage + Send + Sync {
    async fn create_scp(&self, command: CreateScpCommand) -> Result<Policy, CreateScpError>;
    async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError>;
    async fn update_scp(&self, command: UpdateScpCommand) -> Result<Policy, UpdateScpError>;
    async fn get_scp(&self, query: GetScpQuery) -> Result<Policy, GetScpError>;
    async fn list_scps(&self, query: ListScpsQuery) -> Result<Vec<Policy>, ListScpsError>;
}
