use crate::shared::domain::Policy;
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::dto::{CreateScpCommand, DeleteScpCommand, UpdateScpCommand, GetScpQuery, ListScpsQuery};
use crate::features::create_scp::error::{CreateScpError, DeleteScpError, UpdateScpError, GetScpError, ListScpsError};
use crate::shared::domain::ports::{PolicyStorage, PolicyStorageError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct MockScpPersister {
    scps: Arc<Mutex<HashMap<String, Policy>>>,
}

impl MockScpPersister {
    pub fn new() -> Self {
        Self {
            scps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_scps(scps: HashMap<String, Policy>) -> Self {
        Self {
            scps: Arc::new(Mutex::new(scps)),
        }
    }
}

#[async_trait]
impl PolicyStorage for MockScpPersister {
    async fn save(&self, policy: Policy) -> Result<(), PolicyStorageError> {
        let mut scps = self.scps.lock().await;
        scps.insert(policy.id.to_string(), policy);
        Ok(())
    }

    async fn delete(&self, policy_id: &str) -> Result<(), PolicyStorageError> {
        let mut scps = self.scps.lock().await;
        scps.remove(policy_id);
        Ok(())
    }

    async fn update(&self, policy: Policy) -> Result<(), PolicyStorageError> {
        let mut scps = self.scps.lock().await;
        scps.insert(policy.id.to_string(), policy);
        Ok(())
    }

    async fn get(&self, policy_id: &str) -> Result<Option<Policy>, PolicyStorageError> {
        let scps = self.scps.lock().await;
        Ok(scps.get(policy_id).cloned())
    }

    async fn list(&self, _limit: Option<u32>, _offset: Option<u32>) -> Result<Vec<Policy>, PolicyStorageError> {
        let scps = self.scps.lock().await;
        Ok(scps.values().cloned().collect())
    }
}

#[async_trait]
impl ScpPersister for MockScpPersister {
    async fn create_scp(&self, command: CreateScpCommand) -> Result<Policy, CreateScpError> {
        let mut scps = self.scps.lock().await;
        if scps.contains_key(&command.scp_id) {
            return Err(CreateScpError::ScpAlreadyExists);
        }
        
        let policy = Policy {
            id: crate::shared::domain::Hrn::new("organizations", "scp", &command.scp_id),
            content: command.scp_content,
            description: command.description,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        
        scps.insert(command.scp_id.clone(), policy.clone());
        Ok(policy)
    }

    async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
        let mut scps = self.scps.lock().await;
        if !scps.contains_key(&command.scp_id) {
            return Err(DeleteScpError::ScpNotFound);
        }
        
        scps.remove(&command.scp_id);
        Ok(())
    }

    async fn update_scp(&self, command: UpdateScpCommand) -> Result<Policy, UpdateScpError> {
        let mut scps = self.scps.lock().await;
        let policy = scps.get_mut(&command.scp_id)
            .ok_or(UpdateScpError::ScpNotFound)?;
        
        policy.content = command.scp_content;
        policy.description = command.description;
        policy.updated_at = chrono::Utc::now();
        
        Ok(policy.clone())
    }

    async fn get_scp(&self, query: GetScpQuery) -> Result<Policy, GetScpError> {
        let scps = self.scps.lock().await;
        scps.get(&query.scp_id)
            .cloned()
            .ok_or(GetScpError::ScpNotFound)
    }

    async fn list_scps(&self, query: ListScpsQuery) -> Result<Vec<Policy>, ListScpsError> {
        let scps = self.scps.lock().await;
        let mut result: Vec<Policy> = scps.values().cloned().collect();
        
        // Apply limit and offset if specified
        if let Some(offset) = query.offset {
            result = result.into_iter().skip(offset as usize).collect();
        }
        
        if let Some(limit) = query.limit {
            result = result.into_iter().take(limit as usize).collect();
        }
        
        Ok(result)
    }
}
