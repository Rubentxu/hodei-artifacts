// Feature gate is already applied at the module level in mod.rs

use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

#[derive(Clone)]
pub struct SurrealEmbeddedStorage {
    db: Surreal<Db>,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealEmbeddedStorage {
    /// path: filesystem path for RocksDB directory
    pub async fn new(namespace: &str, database: &str, path: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<RocksDb>(path)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            table: "policies".into(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealEmbeddedStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord {
                src: policy.to_string(),
            })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let res: Option<PolicyRecord> = self
            .db
            .delete((self.table.as_str(), id))
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
    }

    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError> {
        let thing = (self.table.as_str(), id);
        let rec: Option<PolicyRecord> = self
            .db
            .select(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;

        match rec {
            Some(r) => {
                let policy = r
                    .src
                    .parse::<Policy>()
                    .map_err(|e| StorageError::ParsingError(e.to_string()))?;
                Ok(Some(policy))
            }
            None => Ok(None),
        }
    }

    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        let recs: Vec<PolicyRecord> = self
            .db
            .select(self.table.as_str())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        let mut out = Vec::with_capacity(recs.len());
        for r in recs {
            let p = r
                .src
                .parse::<Policy>()
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
            out.push(p);
        }
        Ok(out)
    }
}
