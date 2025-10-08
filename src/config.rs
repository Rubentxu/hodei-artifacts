//! Configuration module for Hodei Artifacts API
//!
//! This module handles loading and managing application configuration
//! from environment variables, configuration files, and defaults.

use serde::{Deserialize, Serialize};
use std::env;

/// Application configuration
///
/// This struct holds all configuration parameters for the Hodei Artifacts API.
/// Configuration can be loaded from environment variables, configuration files,
/// or use sensible defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server configuration
    pub server: ServerConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Schema configuration
    pub schema: SchemaConfig,

    /// Logging configuration
    pub logging: LoggingConfig,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to (default: 0.0.0.0)
    pub host: String,

    /// Port to bind to (default: 3000)
    pub port: u16,

    /// Request timeout in seconds (default: 30)
    pub request_timeout_secs: u64,

    /// Maximum request body size in bytes (default: 10MB)
    pub max_body_size: usize,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database type (e.g., "surrealdb", "in-memory")
    pub db_type: String,

    /// Database connection URL
    pub url: String,

    /// Database namespace
    pub namespace: Option<String>,

    /// Database name
    pub database: Option<String>,

    /// Connection pool size (default: 10)
    pub pool_size: u32,
}

/// Schema configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    /// Whether to register IAM schema on startup (default: true)
    pub register_iam_on_startup: bool,

    /// Specific schema version to use (optional)
    pub version: Option<String>,

    /// Whether to validate schemas after building (default: true)
    pub validate: bool,

    /// Schema storage type (e.g., "in-memory", "surrealdb")
    pub storage_type: String,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (default: "info")
    /// Valid values: "trace", "debug", "info", "warn", "error"
    pub level: String,

    /// Log format (default: "pretty")
    /// Valid values: "pretty", "json", "compact"
    pub format: String,

    /// Whether to include timestamps (default: true)
    pub include_timestamps: bool,

    /// Whether to include file/line information (default: false)
    pub include_location: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            schema: SchemaConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            request_timeout_secs: 30,
            max_body_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            db_type: "surrealdb".to_string(),
            url: "memory://".to_string(),
            namespace: Some("hodei".to_string()),
            database: Some("artifacts".to_string()),
            pool_size: 10,
        }
    }
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            register_iam_on_startup: false,
            version: None,
            validate: true,
            storage_type: "surrealdb".to_string(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
            include_timestamps: true,
            include_location: false,
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    ///
    /// This method loads configuration from environment variables with sensible defaults.
    /// Environment variables follow the pattern: HODEI_<SECTION>_<KEY>
    ///
    /// # Examples
    ///
    /// - `HODEI_SERVER_HOST=127.0.0.1`
    /// - `HODEI_SERVER_PORT=8080`
    /// - `HODEI_DATABASE_URL=ws://localhost:8000`
    /// - `HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP=false`
    /// - `HODEI_LOGGING_LEVEL=debug`
    ///
    /// # Returns
    ///
    /// A Config instance with values loaded from environment or defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Server configuration
        if let Ok(host) = env::var("HODEI_SERVER_HOST") {
            config.server.host = host;
        }
        if let Ok(port) = env::var("HODEI_SERVER_PORT") {
            if let Ok(port) = port.parse() {
                config.server.port = port;
            }
        }
        if let Ok(timeout) = env::var("HODEI_SERVER_REQUEST_TIMEOUT_SECS") {
            if let Ok(timeout) = timeout.parse() {
                config.server.request_timeout_secs = timeout;
            }
        }
        if let Ok(max_size) = env::var("HODEI_SERVER_MAX_BODY_SIZE") {
            if let Ok(max_size) = max_size.parse() {
                config.server.max_body_size = max_size;
            }
        }

        // Database configuration
        if let Ok(db_type) = env::var("HODEI_DATABASE_TYPE") {
            config.database.db_type = db_type;
        }
        if let Ok(url) = env::var("HODEI_DATABASE_URL") {
            config.database.url = url;
        }
        if let Ok(ns) = env::var("HODEI_DATABASE_NAMESPACE") {
            config.database.namespace = Some(ns);
        }
        if let Ok(db) = env::var("HODEI_DATABASE_NAME") {
            config.database.database = Some(db);
        }
        if let Ok(pool_size) = env::var("HODEI_DATABASE_POOL_SIZE") {
            if let Ok(pool_size) = pool_size.parse() {
                config.database.pool_size = pool_size;
            }
        }

        // Schema configuration
        if let Ok(register) = env::var("HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP") {
            config.schema.register_iam_on_startup =
                register.to_lowercase() == "true" || register == "1";
        }
        if let Ok(version) = env::var("HODEI_SCHEMA_VERSION") {
            config.schema.version = Some(version);
        }
        if let Ok(validate) = env::var("HODEI_SCHEMA_VALIDATE") {
            config.schema.validate = validate.to_lowercase() == "true" || validate == "1";
        }
        if let Ok(storage) = env::var("HODEI_SCHEMA_STORAGE_TYPE") {
            config.schema.storage_type = storage;
        }

        // Logging configuration
        if let Ok(level) = env::var("HODEI_LOGGING_LEVEL") {
            config.logging.level = level;
        }
        if let Ok(format) = env::var("HODEI_LOGGING_FORMAT") {
            config.logging.format = format;
        }
        if let Ok(timestamps) = env::var("HODEI_LOGGING_INCLUDE_TIMESTAMPS") {
            config.logging.include_timestamps =
                timestamps.to_lowercase() == "true" || timestamps == "1";
        }
        if let Ok(location) = env::var("HODEI_LOGGING_INCLUDE_LOCATION") {
            config.logging.include_location = location.to_lowercase() == "true" || location == "1";
        }

        config
    }

    /// Validate the configuration
    ///
    /// This method checks if the configuration values are valid and consistent.
    ///
    /// # Returns
    ///
    /// Ok(()) if configuration is valid, Err with a description if invalid
    pub fn validate(&self) -> Result<(), String> {
        // Validate server config
        if self.server.port == 0 {
            return Err("Server port cannot be 0".to_string());
        }
        if self.server.request_timeout_secs == 0 {
            return Err("Request timeout cannot be 0".to_string());
        }
        if self.server.max_body_size == 0 {
            return Err("Max body size cannot be 0".to_string());
        }

        // Validate database config
        if self.database.db_type.is_empty() {
            return Err("Database type cannot be empty".to_string());
        }
        if self.database.url.is_empty() {
            return Err("Database URL cannot be empty".to_string());
        }
        if self.database.pool_size == 0 {
            return Err("Database pool size cannot be 0".to_string());
        }

        // Validate schema config
        if self.schema.storage_type.is_empty() {
            return Err("Schema storage type cannot be empty".to_string());
        }

        // Validate logging config
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            return Err(format!(
                "Invalid log level '{}'. Valid values: {}",
                self.logging.level,
                valid_levels.join(", ")
            ));
        }

        let valid_formats = ["pretty", "json", "compact"];
        if !valid_formats.contains(&self.logging.format.as_str()) {
            return Err(format!(
                "Invalid log format '{}'. Valid values: {}",
                self.logging.format,
                valid_formats.join(", ")
            ));
        }

        Ok(())
    }

    /// Get the server bind address
    ///
    /// Returns a string in the format "host:port"
    pub fn server_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.db_type, "surrealdb");
        assert!(config.schema.register_iam_on_startup);
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_config_validation() {
        let config = Config::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = Config::default();
        invalid_config.server.port = 0;
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = Config::default();
        invalid_config.logging.level = "invalid".to_string();
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_server_address() {
        let config = Config::default();
        assert_eq!(config.server_address(), "0.0.0.0:3000");

        let mut config = Config::default();
        config.server.host = "127.0.0.1".to_string();
        config.server.port = 8080;
        assert_eq!(config.server_address(), "127.0.0.1:8080");
    }

    #[test]
    fn test_from_env() {
        // Set some environment variables
        unsafe {
            env::set_var("HODEI_SERVER_PORT", "8080");
            env::set_var("HODEI_LOGGING_LEVEL", "debug");
            env::set_var("HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP", "false");
        }

        let config = Config::from_env();

        assert_eq!(config.server.port, 8080);
        assert_eq!(config.logging.level, "debug");
        assert!(!config.schema.register_iam_on_startup);

        // Clean up
        unsafe {
            env::remove_var("HODEI_SERVER_PORT");
            env::remove_var("HODEI_LOGGING_LEVEL");
            env::remove_var("HODEI_SCHEMA_REGISTER_IAM_ON_STARTUP");
        }
    }
}
