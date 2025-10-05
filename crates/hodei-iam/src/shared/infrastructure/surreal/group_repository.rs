use crate::shared::application::ports::{GroupRepository, GroupRepositoryError};
use crate::shared::domain::Group;
use async_trait::async_trait;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use tracing::error;

/// SurrealDB implementation of GroupRepository
pub struct SurrealGroupRepository {
    db: Surreal<Any>,
}

impl SurrealGroupRepository {
    /// Create a new SurrealGroupRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupRepository for SurrealGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), GroupRepositoryError> {
        let thing: RecordId = ("groups", group.hrn.to_string()).try_into().map_err(|e| {
            error!("Failed to create RecordId for group {}: {}", group.hrn, e);
            GroupRepositoryError::InvalidHrn(group.hrn.to_string())
        })?;

        let _: surrealdb::opt::IntoRecordId =
            self.db.create(thing).content(group).await.map_err(|e| {
                error!("Database error while saving group {}: {}", group.hrn, e);
                GroupRepositoryError::DatabaseError(e.to_string())
            })?;

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, GroupRepositoryError> {
        let thing: RecordId = ("groups", hrn.to_string()).try_into().map_err(|e| {
            error!("Failed to create RecordId for hrn {}: {}", hrn, e);
            GroupRepositoryError::InvalidHrn(hrn.to_string())
        })?;

        let group: Option<Group> = self.db.select(thing).await.map_err(|e| {
            error!("Database error while finding group {}: {}", hrn, e);
            GroupRepositoryError::DatabaseError(e.to_string())
        })?;

        Ok(group)
    }

    async fn find_all(&self) -> Result<Vec<Group>, GroupRepositoryError> {
        let groups: Vec<Group> = self.db.select("groups").await.map_err(|e| {
            error!("Database error while finding all groups: {}", e);
            GroupRepositoryError::DatabaseError(e.to_string())
        })?;

        Ok(groups)
    }
}
