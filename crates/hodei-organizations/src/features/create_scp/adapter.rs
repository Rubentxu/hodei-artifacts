use crate::features::create_scp::dto::{
    CreateScpCommand, DeleteScpCommand, GetScpQuery, ListScpsQuery, ScpDto, UpdateScpCommand,
};
use crate::features::create_scp::error::{
    CreateScpError, DeleteScpError, GetScpError, ListScpsError, UpdateScpError,
};
use crate::features::create_scp::ports::ScpPersister;
use crate::internal::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use kernel::Hrn;
use surrealdb::engine::any::Any;
use surrealdb::{RecordId, Surreal};
use tracing::instrument;

/// SurrealDB implementation of `ScpPersister` trait
pub struct SurrealScpPersister {
    db: Surreal<Any>,
}

impl SurrealScpPersister {
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }

    fn record_id(hrn: &Hrn) -> RecordId {
        RecordId::from(("scp", hrn.to_string().as_str()))
    }

    fn to_dto(scp: ServiceControlPolicy) -> ScpDto {
        ScpDto {
            hrn: scp.hrn,
            name: scp.name,
            document: scp.document,
        }
    }
}

#[async_trait]
impl ScpPersister for SurrealScpPersister {
    #[instrument(skip(self, command), fields(hrn = %command.hrn))]
    async fn create_scp(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
        let record_id = Self::record_id(&command.hrn);

        // Check if SCP already exists
        if let Some(existing) = self
            .db
            .select::<Option<ServiceControlPolicy>>(record_id.clone())
            .await
            .map_err(|e| CreateScpError::StorageError(e.to_string()))?
        {
            return Err(CreateScpError::ScpAlreadyExists(existing.hrn.to_string()));
        }

        let scp = ServiceControlPolicy {
            hrn: command.hrn.clone(),
            name: command.name.clone(),
            document: command.document.clone(),
        };

        let _created: Option<ServiceControlPolicy> = self
            .db
            .create(record_id)
            .content(scp.clone())
            .await
            .map_err(|e| CreateScpError::StorageError(e.to_string()))?;

        Ok(Self::to_dto(scp))
    }

    #[instrument(skip(self, command), fields(hrn = %command.hrn))]
    async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
        let record_id = Self::record_id(&command.hrn);

        // Ensure exists before deleting
        let existing = self
            .db
            .select::<Option<ServiceControlPolicy>>(record_id.clone())
            .await
            .map_err(|e| DeleteScpError::StorageError(e.to_string()))?;

        if existing.is_none() {
            return Err(DeleteScpError::ScpNotFound(command.hrn.to_string()));
        }

        let _deleted: Option<ServiceControlPolicy> = self
            .db
            .delete(record_id)
            .await
            .map_err(|e| DeleteScpError::StorageError(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, command), fields(hrn = %command.hrn))]
    async fn update_scp(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
        let record_id = Self::record_id(&command.hrn);

        let mut scp = self
            .db
            .select::<Option<ServiceControlPolicy>>(record_id.clone())
            .await
            .map_err(|e| UpdateScpError::StorageError(e.to_string()))?
            .ok_or_else(|| UpdateScpError::ScpNotFound(command.hrn.to_string()))?;

        if let Some(name) = command.name {
            scp.name = name;
        }

        if let Some(document) = command.document {
            scp.document = document;
        }

        let updated: Option<ServiceControlPolicy> = self
            .db
            .update(record_id)
            .content(scp.clone())
            .await
            .map_err(|e| UpdateScpError::StorageError(e.to_string()))?;

        Ok(Self::to_dto(updated.unwrap_or(scp)))
    }

    #[instrument(skip(self, query), fields(hrn = %query.hrn))]
    async fn get_scp(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError> {
        let record_id = Self::record_id(&query.hrn);

        let scp = self
            .db
            .select::<Option<ServiceControlPolicy>>(record_id)
            .await
            .map_err(|e| GetScpError::StorageError(e.to_string()))?
            .ok_or_else(|| GetScpError::ScpNotFound(query.hrn.to_string()))?;

        Ok(Self::to_dto(scp))
    }

    #[instrument(skip(self, query))]
    async fn list_scps(&self, query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
        let mut scps: Vec<ServiceControlPolicy> = self
            .db
            .select("scp")
            .await
            .map_err(|e| ListScpsError::StorageError(e.to_string()))?;

        if let Some(offset) = query.offset {
            if offset as usize >= scps.len() {
                scps.clear();
            } else {
                scps = scps.split_off(offset as usize);
            }
        }

        if let Some(limit) = query.limit
            && scps.len() > limit as usize {
                scps.truncate(limit as usize);
            }

        Ok(scps.into_iter().map(Self::to_dto).collect())
    }
}
