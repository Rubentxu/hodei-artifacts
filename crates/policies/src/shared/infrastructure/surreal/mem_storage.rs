use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

#[derive(Clone)]
pub struct SurrealMemStorage {
    db: Surreal<Db>,
    _namespace: String,
    _database: String,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealMemStorage {
    pub async fn new(namespace: &str, database: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            _namespace: namespace.to_string(),
            _database: database.to_string(),
            table: "policies".to_string(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealMemStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord { src: policy.to_string() })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let thing = (self.table.as_str(), id);
        let res: Option<PolicyRecord> = self
            .db
            .delete(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn can_save_and_load_policy() {
        let storage = SurrealMemStorage::new("test_ns", "test_db")
            .await
            .expect("connect mem surreal");
        let src = r#"permit(principal, action, resource);"#;
        let p: Policy = src.parse().expect("parse policy");
        storage.save_policy(&p).await.expect("save");
        let all = storage.load_all_policies().await.expect("load");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].to_string(), p.to_string());
        let removed = storage.delete_policy(&p.id().to_string()).await.expect("delete");
        assert!(removed);
    }
}
