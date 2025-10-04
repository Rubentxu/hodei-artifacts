use crate::shared::application::ports::UserRepository;
use crate::shared::domain::User;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use async_trait::async_trait;

/// SurrealDB implementation of UserRepository
pub struct SurrealUserRepository {
    db: Surreal<Any>,
}

impl SurrealUserRepository {
    /// Create a new SurrealUserRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserRepository for SurrealUserRepository {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("users", user.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(user).await?;
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error> {
        let thing: RecordId = ("users", hrn.to_string()).try_into()?;
        let user: Option<User> = self.db.select(thing).await?;
        Ok(user)
    }

    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error> {
        let users: Vec<User> = self.db.select("users").await?;
        Ok(users)
    }
}
