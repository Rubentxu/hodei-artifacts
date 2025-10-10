//! Configuration module for Hodei Artifacts API
//!
//! This module handles loading and managing application configuration
//! from multiple sources with hierarchical precedence and validation.

use ::config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::env;

/// Main application configuration
///
/// This struct holds all configuration parameters for the Hodei Artifacts API.
/// Configuration is loaded from multiple sources with the following precedence:
/// 1. Environment variables (HODEI_ prefix)
/// 2. config/local.toml
/// 3. config/{RUN_MODE}.toml
/// 4. config/default.toml
/// 5. Hardcoded defaults
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Schema configuration
    pub schema: SchemaConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// RocksDB specific configuration
    pub rocksdb: RocksDbConfig,
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
    /// Database type (only "rocksdb" supported)
    pub db_type: String,

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
    /// Whether to register IAM schema on startup (default: false)
    pub register_iam_on_startup: bool,

    /// Specific schema version to use (optional)
    pub version: Option<String>,

    /// Whether to validate schemas after building (default: true)
    pub validate: bool,

    /// Schema storage type (default: "rocksdb")
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

/// RocksDB specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocksDbConfig {
    /// Path to RocksDB database file (default: "./data/hodei.rocksdb")
    pub path: String,

    /// Create database if it doesn't exist (default: true)
    pub create_if_missing: bool,

    /// Enable compression (default: true)
    pub compression: bool,

    /// Maximum number of open files (default: 1000)
    pub max_open_files: i32,

    /// Write buffer size in bytes (default: 64MB)
    pub write_buffer_size: usize,
}

// Default derived for AppConfig

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
            db_type: "rocksdb".to_string(),
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
            storage_type: "rocksdb".to_string(),
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

impl Default for RocksDbConfig {
    fn default() -> Self {
        Self {
            path: "./target/debug/data/hodei.rocksdb".to_string(),
            create_if_missing: true,
            compression: true,
            max_open_files: 1000,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
        }
    }
}

impl AppConfig {
    /// Load configuration from multiple sources with hierarchical precedence
    ///
    /// Sources (in order of precedence, highest first):
    /// 1. Environment variables (HODEI_ prefix)
    /// 2. config/local.toml
    /// 3. config/{RUN_MODE}.toml
    /// 4. config/default.toml
    /// 5. Hardcoded defaults
    ///
    /// # Returns
    ///
    /// A validated AppConfig instance or a ConfigError with clear messages
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let s = Config::builder()
            // Default values (lowest precedence)
            .add_source(File::with_name("config/default").required(false))
            // Environment-specific configuration
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Local overrides (higher precedence)
            .add_source(File::with_name("config/local").required(false))
            // Environment variables with HODEI_ prefix (highest precedence)
            .add_source(Environment::with_prefix("HODEI").separator("__"))
            .build()?;

        let app_config: AppConfig = s.try_deserialize()?;

        // Validate the configuration
        app_config.validate()?;

        Ok(app_config)
    }

    /// Validate the entire configuration
    ///
    /// # Returns
    ///
    /// Ok(()) if configuration is valid, ConfigError with clear message if invalid
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.server.validate()?;
        self.database.validate()?;
        self.rocksdb.validate()?;
        self.logging.validate()?;
        Ok(())
    }

    /// Get the server bind address
    ///
    /// Returns a string in the format "host:port"
    pub fn server_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

impl ServerConfig {
    /// Validate server configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::Message(
                "Server port cannot be 0. Please set HODEI_SERVER__PORT to a valid port number (1-65535)".to_string()
            ));
        }

        if self.host.is_empty() {
            return Err(ConfigError::Message(
                "Server host cannot be empty. Please set HODEI_SERVER__HOST".to_string(),
            ));
        }

        if self.request_timeout_secs == 0 {
            return Err(ConfigError::Message(
                "Request timeout cannot be 0. Please set HODEI_SERVER__REQUEST_TIMEOUT_SECS to a positive value".to_string()
            ));
        }

        if self.max_body_size == 0 {
            return Err(ConfigError::Message(
                "Max body size cannot be 0. Please set HODEI_SERVER__MAX_BODY_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }
}

impl DatabaseConfig {
    /// Validate database configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.db_type != "rocksdb" {
            return Err(ConfigError::Message(format!(
                "Unsupported database type '{}'. Only 'rocksdb' is supported. Please set HODEI_DATABASE__DB_TYPE to 'rocksdb'",
                self.db_type
            )));
        }

        if self.namespace.is_none() || self.namespace.as_ref().unwrap().is_empty() {
            return Err(ConfigError::Message(
                "Database namespace cannot be empty. Please set HODEI_DATABASE__NAMESPACE"
                    .to_string(),
            ));
        }

        if self.database.is_none() || self.database.as_ref().unwrap().is_empty() {
            return Err(ConfigError::Message(
                "Database name cannot be empty. Please set HODEI_DATABASE__DATABASE".to_string(),
            ));
        }

        if self.pool_size == 0 {
            return Err(ConfigError::Message(
                "Database pool size cannot be 0. Please set HODEI_DATABASE__POOL_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }
}

impl RocksDbConfig {
    /// Validate RocksDB configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.path.is_empty() {
            return Err(ConfigError::Message(
                "RocksDB path cannot be empty. Please set HODEI_ROCKSDB__PATH to a valid file path"
                    .to_string(),
            ));
        }

        // RocksDB path is actually a directory that contains the database files
        let path = std::path::Path::new(&self.path);

        if path.exists() {
            // Case 1: Path already exists
            // It must be a directory (RocksDB creates and uses directories)
            if !path.is_dir() {
                return Err(ConfigError::Message(format!(
                    "RocksDB path '{}' exists but is not a directory. RocksDB requires a directory path. Please remove this file or choose a different path.",
                    self.path
                )));
            }

            // Verify the directory is writable
            let test_file = path.join(".hodei_test_write");
            match std::fs::write(&test_file, "test") {
                Ok(_) => {
                    // Clean up test file
                    let _ = std::fs::remove_file(test_file);
                }
                Err(e) => {
                    return Err(ConfigError::Message(format!(
                        "RocksDB directory '{}' exists but is not writable: {}. Please check permissions.",
                        path.display(),
                        e
                    )));
                }
            }
        } else {
            // Case 2: Path doesn't exist yet (will be created by RocksDB)
            // Validate that we can create it by checking parent directory
            let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));

            if !parent.exists() {
                // Parent doesn't exist, try to create it
                if self.create_if_missing {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return Err(ConfigError::Message(format!(
                            "Cannot create parent directory '{}' for RocksDB: {}. Please check permissions or set HODEI_ROCKSDB__PATH to a writable location.",
                            parent.display(),
                            e
                        )));
                    }
                } else {
                    return Err(ConfigError::Message(format!(
                        "Parent directory '{}' does not exist and create_if_missing is false. Please create the directory manually or set create_if_missing=true.",
                        parent.display()
                    )));
                }
            } else {
                // Parent exists, verify it's writable
                let test_file = parent.join(".hodei_test_write");
                match std::fs::write(&test_file, "test") {
                    Ok(_) => {
                        // Clean up test file
                        let _ = std::fs::remove_file(test_file);
                    }
                    Err(e) => {
                        return Err(ConfigError::Message(format!(
                            "Parent directory '{}' is not writable: {}. Please check permissions.",
                            parent.display(),
                            e
                        )));
                    }
                }
            }
        }

        if self.max_open_files <= 0 {
            return Err(ConfigError::Message(
                "RocksDB max_open_files must be positive. Please set HODEI_ROCKSDB__MAX_OPEN_FILES to a value > 0".to_string()
            ));
        }

        if self.write_buffer_size == 0 {
            return Err(ConfigError::Message(
                "RocksDB write_buffer_size cannot be 0. Please set HODEI_ROCKSDB__WRITE_BUFFER_SIZE to a positive value".to_string()
            ));
        }

        Ok(())
    }
}

impl LoggingConfig {
    /// Validate logging configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.level.as_str()) {
            return Err(ConfigError::Message(format!(
                "Invalid log level '{}'. Valid values: {}. Please set HODEI_LOGGING__LEVEL to one of these",
                self.level,
                valid_levels.join(", ")
            )));
        }

        let valid_formats = ["pretty", "json", "compact"];
        if !valid_formats.contains(&self.format.as_str()) {
            return Err(ConfigError::Message(format!(
                "Invalid log format '{}'. Valid values: {}. Please set HODEI_LOGGING__FORMAT to one of these",
                self.format,
                valid_formats.join(", ")
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.db_type, "rocksdb");
        assert!(!config.schema.register_iam_on_startup);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.rocksdb.path, "./target/debug/data/hodei.rocksdb");
    }

    #[test]
    fn test_config_validation() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = AppConfig::default();
        invalid_config.server.port = 0;
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = AppConfig::default();
        invalid_config.database.db_type = "postgres".to_string();
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = AppConfig::default();
        invalid_config.logging.level = "invalid".to_string();
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_server_address() {
        let config = AppConfig::default();
        assert_eq!(config.server_address(), "0.0.0.0:3000");

        let mut config = AppConfig::default();
        config.server.host = "127.0.0.1".to_string();
        config.server.port = 8080;
        assert_eq!(config.server_address(), "127.0.0.1:8080");
    }

    #[test]
    fn test_rocksdb_validation() {
        let config = RocksDbConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = RocksDbConfig::default();
        invalid_config.path = "".to_string();
        assert!(invalid_config.validate().is_err());

        let mut invalid_config = RocksDbConfig::default();
        invalid_config.max_open_files = 0;
        assert!(invalid_config.validate().is_err());
    }
}
