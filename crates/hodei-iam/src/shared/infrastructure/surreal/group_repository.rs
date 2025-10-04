use crate::shared::application::ports::GroupRepository;
use crate::shared::domain::Group;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use async_trait::async_trait;

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
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("groups", group.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(group).await?;
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error> {
        let thing: RecordId = ("groups", hrn.to_string()).try_into()?;
        let group: Option<Group> = self.db.select(thing).await?;
        Ok(group)
    }

    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error> {
        let groups: Vec<Group> = self.db.select("groups").await?;
        Ok(groups)
    }
}
