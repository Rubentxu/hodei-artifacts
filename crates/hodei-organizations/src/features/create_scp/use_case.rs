use crate::shared::domain::{Hrn, Policy};
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::dto::{CreateScpCommand, DeleteScpCommand, UpdateScpCommand, GetScpQuery, ListScpsQuery, ScpDto};
use crate::features::create_scp::error::{CreateScpError, DeleteScpError, UpdateScpError, GetScpError, ListScpsError};
use async_trait::async_trait;
use chrono::Utc;
use cedar_policy::{PolicyId, PolicySet};
use tracing::instrument;

pub struct CreateScpUseCase<T: ScpPersister> {
    scp_persister: T,
}

impl<T: ScpPersister> CreateScpUseCase<T> {
    pub fn new(scp_persister: T) -> Self {
        Self { scp_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
        // Validate SCP content
        let policy_set = PolicySet::from_str(&command.scp_content)
            .map_err(|_| CreateScpError::InvalidScpContent)?;
        
        if policy_set.policies().count() != 1 {
            return Err(CreateScpError::InvalidScpContent);
        }

        // Create SCP entity
        let policy_id = PolicyId::from_str(&command.scp_id)
            .map_err(|_| CreateScpError::InvalidScpContent)?;
        
        let hrn = Hrn::new("organizations", "scp", &command.scp_id);
        let now = Utc::now();
        
        let policy = Policy {
            id: hrn.clone(),
            content: command.scp_content,
            description: command.description,
            created_at: now,
            updated_at: now,
        };

        // Persist SCP
        let saved_policy = self.scp_persister.create_scp(command).await?;
        Ok(ScpDto::from(saved_policy))
    }
}

pub struct DeleteScpUseCase<T: ScpPersister> {
    scp_persister: T,
}

impl<T: ScpPersister> DeleteScpUseCase<T> {
    pub fn new(scp_persister: T) -> Self {
        Self { scp_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
        self.scp_persister.delete_scp(command).await
    }
}

pub struct UpdateScpUseCase<T: ScpPersister> {
    scp_persister: T,
}

impl<T: ScpPersister> UpdateScpUseCase<T> {
    pub fn new(scp_persister: T) -> Self {
        Self { scp_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
        // Validate SCP content
        let policy_set = PolicySet::from_str(&command.scp_content)
            .map_err(|_| UpdateScpError::InvalidScpContent)?;
        
        if policy_set.policies().count() != 1 {
            return Err(UpdateScpError::InvalidScpContent);
        }

        // Update SCP
        let updated_policy = self.scp_persister.update_scp(command).await?;
        Ok(ScpDto::from(updated_policy))
    }
}

pub struct GetScpUseCase<T: ScpPersister> {
    scp_persister: T,
}

impl<T: ScpPersister> GetScpUseCase<T> {
    pub fn new(scp_persister: T) -> Self {
        Self { scp_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError> {
        let policy = self.scp_persister.get_scp(query).await?;
        Ok(ScpDto::from(policy))
    }
}

pub struct ListScpsUseCase<T: ScpPersister> {
    scp_persister: T,
}

impl<T: ScpPersister> ListScpsUseCase<T> {
    pub fn new(scp_persister: T) -> Self {
        Self { scp_persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
        let policies = self.scp_persister.list_scps(query).await?;
        Ok(policies.into_iter().map(ScpDto::from).collect())
    }
}
