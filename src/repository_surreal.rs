//! Durable SurrealDB adapter for the repository ports.
//!
//! The `repository` table has a unique index on `hrn`, so the database, not
//! the process, enforces uniqueness across restarts and concurrent writers.

use async_trait::async_trait;
use kernel::Hrn;
use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

use crate::repository::{
    RepositoryReadError, RepositoryReader, RepositoryView, RepositoryWriteError, RepositoryWriter,
};

#[derive(Debug, Clone)]
pub struct SurrealRepositoryStore {
    db: Surreal<Db>,
}

#[derive(Debug, Deserialize)]
struct Row {
    hrn: String,
    account: String,
    name: String,
    description: Option<String>,
}

impl Row {
    fn into_view(self) -> RepositoryView {
        RepositoryView::new(self.hrn, self.account, self.name, self.description)
    }
}

impl SurrealRepositoryStore {
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }

    /// Create the table, fields and the uniqueness index. Idempotent.
    pub async fn initialize(&self) -> Result<(), RepositoryReadError> {
        self.db
            .query(
                r#"
                DEFINE TABLE IF NOT EXISTS repository SCHEMAFULL;
                DEFINE FIELD IF NOT EXISTS hrn ON repository TYPE string;
                DEFINE FIELD IF NOT EXISTS account ON repository TYPE string;
                DEFINE FIELD IF NOT EXISTS name ON repository TYPE string;
                DEFINE FIELD IF NOT EXISTS description ON repository TYPE option<string>;
                DEFINE INDEX IF NOT EXISTS repository_hrn_unique ON repository FIELDS hrn UNIQUE;
                "#,
            )
            .await
            .map_err(|_| RepositoryReadError::Unavailable)?
            .check()
            .map_err(|_| RepositoryReadError::Unavailable)?;
        Ok(())
    }
}

#[async_trait]
impl RepositoryReader for SurrealRepositoryStore {
    async fn find_by_hrn(
        &self,
        account: &str,
        hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        let mut response = self
            .db
            .query("SELECT hrn, account, name, description FROM repository WHERE hrn = $hrn AND account = $account LIMIT 1")
            .bind(("hrn", hrn.to_string()))
            .bind(("account", account.to_string()))
            .await
            .map_err(|_| RepositoryReadError::Unavailable)?;
        let rows: Vec<Row> = response
            .take(0)
            .map_err(|_| RepositoryReadError::Unavailable)?;
        Ok(rows.into_iter().next().map(Row::into_view))
    }
}

#[async_trait]
impl RepositoryWriter for SurrealRepositoryStore {
    async fn create(
        &self,
        account: &str,
        hrn: &Hrn,
        view: RepositoryView,
    ) -> Result<RepositoryView, RepositoryWriteError> {
        if view.account != account || view.hrn != hrn.to_string() {
            return Err(RepositoryWriteError::InvalidRequest);
        }
        let payload = serde_json::json!({
            "hrn": view.hrn,
            "account": view.account,
            "name": view.name,
            "description": view.description,
        });
        let mut response = self
            .db
            .query("CREATE repository CONTENT $data")
            .bind(("data", payload))
            .await
            .map_err(|error| classify(error.to_string()))?;
        let rows: Vec<Row> = response
            .take(0)
            .map_err(|error| classify(error.to_string()))?;
        rows.into_iter()
            .next()
            .map(Row::into_view)
            .ok_or(RepositoryWriteError::Unavailable)
    }
}

fn classify(error: String) -> RepositoryWriteError {
    if error.to_ascii_lowercase().contains("unique")
        || error.contains("already exists")
        || error.contains("read or write conflict")
    {
        RepositoryWriteError::AlreadyExists
    } else {
        RepositoryWriteError::Unavailable
    }
}
