//! SurrealDB infrastructure adapter for shared persistence layer

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Error)]
pub enum SurrealDbError {
    #[error("SurrealDB connection error: {0}")]
    ConnectionError(String),
    
    #[error("SurrealDB query error: {0}")]
    QueryError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Record not found: {0}")]
    RecordNotFound(String),
    
    #[error("Invalid record ID: {0}")]
    InvalidRecordId(String),
}

/// SurrealDB connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurrealDbConfig {
    pub url: String,
    pub namespace: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Default for SurrealDbConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:8000".to_string(),
            namespace: "hodei".to_string(),
            database: "hodei".to_string(),
            username: None,
            password: None,
        }
    }
}

/// SurrealDB connection manager
pub struct SurrealDbConnection {
    config: SurrealDbConfig,
    // Placeholder for actual SurrealDB client
}

impl SurrealDbConnection {
    pub fn new(config: SurrealDbConfig) -> Result<Self, SurrealDbError> {
        Ok(Self { config })
    }

    pub async fn connect(&self) -> Result<(), SurrealDbError> {
        // Placeholder for actual connection logic
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), SurrealDbError> {
        // Placeholder for disconnection logic
        Ok(())
    }
}

/// SurrealDB repository trait for common operations
#[async_trait::async_trait]
pub trait SurrealDbRepository<T>: Send + Sync
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    async fn create(&self, record: T) -> Result<String, SurrealDbError>;
    async fn read(&self, id: &str) -> Result<Option<T>, SurrealDbError>;
    async fn update(&self, id: &str, record: T) -> Result<(), SurrealDbError>;
    async fn delete(&self, id: &str) -> Result<(), SurrealDbError>;
    async fn list(&self, limit: Option<u64>) -> Result<Vec<T>, SurrealDbError>;
}

/// Generic SurrealDB repository implementation
pub struct SurrealDbGenericRepository<T> {
    connection: Arc<SurrealDbConnection>,
    table_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SurrealDbGenericRepository<T> {
    pub fn new(connection: Arc<SurrealDbConnection>, table_name: &str) -> Self {
        Self {
            connection,
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T> SurrealDbRepository<T> for SurrealDbGenericRepository<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    async fn create(&self, record: T) -> Result<String, SurrealDbError> {
        // Placeholder for actual create logic
        Ok("record_id".to_string())
    }

    async fn read(&self, id: &str) -> Result<Option<T>, SurrealDbError> {
        // Placeholder for actual read logic
        Ok(None)
    }

    async fn update(&self, id: &str, record: T) -> Result<(), SurrealDbError> {
        // Placeholder for actual update logic
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), SurrealDbError> {
        // Placeholder for actual delete logic
        Ok(())
    }

    async fn list(&self, limit: Option<u64>) -> Result<Vec<T>, SurrealDbError> {
        // Placeholder for actual list logic
        Ok(Vec::new())
    }
}

/// SurrealDB query builder for complex queries
pub struct SurrealDbQueryBuilder {
    query: String,
    params: Vec<String>,
}

impl SurrealDbQueryBuilder {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            params: Vec::new(),
        }
    }

    pub fn select(mut self, table: &str) -> Self {
        self.query = format!("SELECT * FROM {}", table);
        self
    }

    pub fn where_clause(mut self, condition: &str) -> Self {
        self.query = format!("{} WHERE {}", self.query, condition);
        self
    }

    pub fn order_by(mut self, field: &str, direction: &str) -> Self {
        self.query = format!("{} ORDER BY {} {}", self.query, field, direction);
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.query = format!("{} LIMIT {}", self.query, limit);
        self
    }

    pub fn build(self) -> (String, Vec<String>) {
        (self.query, self.params)
    }
}

/// Common utilities for SurrealDB operations
pub mod utils {
    use super::*;
    use time::OffsetDateTime;

    /// Generate a SurrealDB record ID
    pub fn generate_record_id(table: &str, prefix: &str) -> String {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        format!("{}:{}{}", table, prefix, timestamp)
    }

    /// Convert OffsetDateTime to SurrealDB datetime format
    pub fn to_surreal_datetime(dt: OffsetDateTime) -> String {
        dt.to_rfc3339()
    }

    /// Parse SurrealDB datetime string to OffsetDateTime
    pub fn from_surreal_datetime(dt_str: &str) -> Result<OffsetDateTime, SurrealDbError> {
        OffsetDateTime::parse(dt_str, &time::format_description::well_known::Rfc3339)
            .map_err(|e| SurrealDbError::SerializationError(serde_json::Error::custom(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestRecord {
        id: String,
        name: String,
        created_at: String,
    }

    #[test]
    fn test_config_default() {
        let config = SurrealDbConfig::default();
        assert_eq!(config.url, "ws://localhost:8000");
        assert_eq!(config.namespace, "hodei");
        assert_eq!(config.database, "hodei");
    }

    #[test]
    fn test_query_builder() {
        let (query, params) = SurrealDbQueryBuilder::new()
            .select("users")
            .where_clause("age > 18")
            .order_by("created_at", "DESC")
            .limit(10)
            .build();

        assert!(query.contains("SELECT * FROM users"));
        assert!(query.contains("WHERE age > 18"));
        assert!(query.contains("ORDER BY created_at DESC"));
        assert!(query.contains("LIMIT 10"));
    }

    #[test]
    fn test_utils() {
        let record_id = utils::generate_record_id("test", "prefix_");
        assert!(record_id.starts_with("test:prefix_"));

        let dt = OffsetDateTime::now_utc();
        let dt_str = utils::to_surreal_datetime(dt);
        assert!(!dt_str.is_empty());
    }
}
