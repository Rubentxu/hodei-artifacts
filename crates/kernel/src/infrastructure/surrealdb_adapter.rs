//! SurrealDB infrastructure adapter for shared persistence layer
//!
//! Nota: Este adaptador todavía es una implementación "in-memory / placeholder".
//! Sin embargo, ahora todos los campos previamente marcados como "dead code"
//! (config, connection, table_name) son usados explícitamente para:
//!  - Construir identificadores
//!  - Registrar trazas con `tracing`
//!  - Exponer metadatos de conexión
//!  - Ejecución de tests que validan el comportamiento básico
//!
//! Así eliminamos los warnings de `dead_code` mientras mantenemos la
//! extensibilidad para una futura integración real con SurrealDB.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument};

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

    #[error("Parse error: {0}")]
    ParseError(String),
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
#[derive(Debug, Clone)]
pub struct SurrealDbConnection {
    config: SurrealDbConfig,
    // Futuro: aquí iría el cliente real de SurrealDB
}

impl SurrealDbConnection {
    /// Crea una nueva conexión (aún sin abrir físicamente)
    pub fn new(config: SurrealDbConfig) -> Result<Self, SurrealDbError> {
        debug!(
            url = %config.url,
            ns = %config.namespace,
            db = %config.database,
            "Initializing SurrealDbConnection"
        );
        Ok(Self { config })
    }

    /// Establece la conexión (placeholder)
    #[instrument(level = "debug", skip(self))]
    pub async fn connect(&self) -> Result<(), SurrealDbError> {
        // Uso explícito de los campos para evitar dead_code
        debug!(
            url = %self.config.url,
            ns = %self.config.namespace,
            db = %self.config.database,
            "Connecting to SurrealDB (placeholder)"
        );
        Ok(())
    }

    /// Cierra la conexión (placeholder)
    #[instrument(level = "debug", skip(self))]
    pub async fn disconnect(&self) -> Result<(), SurrealDbError> {
        debug!(
            url = %self.config.url,
            ns = %self.config.namespace,
            db = %self.config.database,
            "Disconnecting from SurrealDB (placeholder)"
        );
        Ok(())
    }

    /// Devuelve el namespace configurado
    pub fn namespace(&self) -> &str {
        &self.config.namespace
    }

    /// Devuelve el nombre de la base de datos configurada
    pub fn database(&self) -> &str {
        &self.config.database
    }

    /// Devuelve la URL de conexión
    pub fn url(&self) -> &str {
        &self.config.url
    }

    /// Devuelve si la conexión tiene credenciales embebidas
    pub fn has_credentials(&self) -> bool {
        self.config.username.is_some() && self.config.password.is_some()
    }

    /// Exponer (lectura) la configuración completa - útil para adaptadores superiores
    pub fn config(&self) -> &SurrealDbConfig {
        &self.config
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
///
/// Conserva *state* (connection + table_name) que ahora se usa en todas las operaciones
pub struct SurrealDbGenericRepository<T> {
    connection: Arc<SurrealDbConnection>,
    table_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SurrealDbGenericRepository<T> {
    pub fn new(connection: Arc<SurrealDbConnection>, table_name: &str) -> Self {
        debug!(
            table = %table_name,
            db = %connection.database(),
            ns = %connection.namespace(),
            "Creating SurrealDbGenericRepository"
        );
        Self {
            connection,
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Devuelve el nombre de la tabla objetivo
    pub fn table(&self) -> &str {
        &self.table_name
    }

    /// Devuelve referencia de la conexión (para adaptadores externos)
    pub fn connection(&self) -> &Arc<SurrealDbConnection> {
        &self.connection
    }
}

#[async_trait::async_trait]
impl<T> SurrealDbRepository<T> for SurrealDbGenericRepository<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    #[instrument(level = "debug", skip(self, _record))]
    async fn create(&self, _record: T) -> Result<String, SurrealDbError> {
        // Usamos table_name y metadata de connection para generar un ID
        let id = utils::generate_record_id(&self.table_name, "rec_");
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Create (placeholder)"
        );
        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    async fn read(&self, id: &str) -> Result<Option<T>, SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Read (placeholder)"
        );
        Ok(None)
    }

    #[instrument(level = "debug", skip(self, _record))]
    async fn update(&self, id: &str, _record: T) -> Result<(), SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Update (placeholder)"
        );
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn delete(&self, id: &str) -> Result<(), SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Delete (placeholder)"
        );
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn list(&self, limit: Option<u64>) -> Result<Vec<T>, SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            ?limit,
            "List (placeholder)"
        );
        Ok(Vec::new())
    }
}

/// SurrealDB query builder for future complex queries
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

impl Default for SurrealDbQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common utilities for SurrealDB operations
pub mod utils {
    use super::*;
    use time::OffsetDateTime;

    /// Generate a SurrealDB record ID (format: table:prefix<timestamp>)
    pub fn generate_record_id(table: &str, prefix: &str) -> String {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        format!("{}:{}{}", table, prefix, timestamp)
    }

    /// Convert OffsetDateTime to SurrealDB datetime format
    pub fn to_surreal_datetime(dt: OffsetDateTime) -> String {
        dt.format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| String::from(""))
    }

    /// Parse SurrealDB datetime string to OffsetDateTime
    pub fn from_surreal_datetime(dt_str: &str) -> Result<OffsetDateTime, SurrealDbError> {
        OffsetDateTime::parse(dt_str, &time::format_description::well_known::Rfc3339)
            .map_err(|e| SurrealDbError::ParseError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use time::OffsetDateTime;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestRecord {
        id: String,
        name: String,
        created_at: String,
    }

    #[tokio::test]
    async fn test_connection_and_repository_usage() {
        let _ = tracing_subscriber::fmt::try_init();

        let config = SurrealDbConfig::default();
        let conn = Arc::new(SurrealDbConnection::new(config.clone()).unwrap());
        conn.connect().await.unwrap();

        assert_eq!(conn.database(), &config.database);
        assert_eq!(conn.namespace(), &config.namespace);
        assert_eq!(conn.url(), &config.url);
        assert!(!conn.has_credentials());

        let repo: SurrealDbGenericRepository<TestRecord> =
            SurrealDbGenericRepository::new(conn.clone(), "test_records");

        assert_eq!(repo.table(), "test_records");
        assert_eq!(repo.connection().database(), "hodei");

        // create -> ensures table_name + connection fields were "read"
        let id = repo
            .create(TestRecord {
                id: "temp".into(),
                name: "example".into(),
                created_at: "now".into(),
            })
            .await
            .unwrap();

        assert!(id.starts_with("test_records:rec_"));

        // list placeholder
        let list = repo.list(Some(10)).await.unwrap();
        assert!(list.is_empty());

        conn.disconnect().await.unwrap();
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
        let (query, _params) = SurrealDbQueryBuilder::new()
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

        let parsed = utils::from_surreal_datetime(&dt_str).unwrap();
        assert!(parsed.unix_timestamp() <= dt.unix_timestamp() + 1);
    }
}
