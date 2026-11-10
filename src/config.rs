//! Configuration module for Hodei Artifacts API
//!
//! This module handles loading and managing application configuration
//! from multiple sources with hierarchical precedence and validation.

use ::config::{Config, ConfigError, Environment, File};
use axum::http::HeaderValue;
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// Use the durable SurrealDB adapter instead of the in-memory slice.
    #[serde(default)]
    pub durable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Security configuration
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub repository: RepositoryConfig,

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

/// Security configuration
///
/// The administrative API is closed by default. `admin_api_enabled` only
/// opens it when the application state has both an authenticator and an
/// authorizer; every request then needs a bearer credential and a Cedar
/// permit.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Whether the `/api/v1` administrative surface is reachable.
    ///
    /// Default: `false`. When false every administrative route is answered
    /// with `403 Forbidden` before any handler runs. The repository routes have
    /// their own credential gate. CORS preflight is answered by the outer CORS
    /// layer and never reaches a handler; unknown routes remain `404`.
    #[serde(default)]
    pub admin_api_enabled: bool,

    /// Browser origins allowed by CORS.
    ///
    /// Default: empty, which means no cross-origin access. Wildcards are
    /// rejected so the permissive policy cannot be reintroduced by
    /// configuration.
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,

    /// HS256 secret or a JWKS file for production JWTs (exactly one).
    #[serde(default, skip_serializing)]
    pub auth_jwt_secret: Option<String>,
    /// Local JWKS used for RS256/ES256 tokens. Never logged or serialized.
    #[serde(default, skip_serializing)]
    pub auth_jwks_path: Option<String>,
    /// Expected JWT issuer.
    #[serde(default)]
    pub auth_issuer: Option<String>,
    /// Expected JWT audience.
    #[serde(default)]
    pub auth_audience: Option<String>,
    /// Cedar policy bundle for production authorization.
    #[serde(default)]
    pub authorization_policy_path: Option<String>,
    /// Cedar schema used to validate the policy bundle.
    #[serde(default)]
    pub authorization_schema_path: Option<String>,
}

impl std::fmt::Debug for SecurityConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecurityConfig")
            .field("admin_api_enabled", &self.admin_api_enabled)
            .field("cors_allowed_origins", &self.cors_allowed_origins)
            .field(
                "auth_jwt_secret",
                &self.auth_jwt_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("auth_jwks_path", &self.auth_jwks_path)
            .field("auth_issuer", &self.auth_issuer)
            .field("auth_audience", &self.auth_audience)
            .field("authorization_policy_path", &self.authorization_policy_path)
            .field("authorization_schema_path", &self.authorization_schema_path)
            .finish()
    }
}

impl SecurityConfig {
    /// Validate only the production auth/authorization bundle.
    pub fn validate_production_auth(&self) -> Result<(), ConfigError> {
        let auth_any = self.auth_jwt_secret.is_some()
            || self.auth_jwks_path.is_some()
            || self.auth_issuer.is_some()
            || self.auth_audience.is_some()
            || self.authorization_policy_path.is_some()
            || self.authorization_schema_path.is_some();
        if auth_any {
            if self.auth_jwt_secret.is_some() == self.auth_jwks_path.is_some() {
                return Err(ConfigError::Message(
                    "Production security requires exactly one of auth_jwt_secret or auth_jwks_path"
                        .to_string(),
                ));
            }
            if self.auth_issuer.is_none()
                || self.auth_audience.is_none()
                || self.authorization_policy_path.is_none()
                || self.authorization_schema_path.is_none()
            {
                return Err(ConfigError::Message(
                    "Production security requires auth_issuer, auth_audience, authorization_policy_path and authorization_schema_path together".to_string(),
                ));
            }
        }
        if self.auth_issuer.as_deref().is_some_and(str::is_empty)
            || self.auth_audience.as_deref().is_some_and(str::is_empty)
            || self.auth_jwt_secret.as_deref().is_some_and(str::is_empty)
            || self.auth_jwks_path.as_deref().is_some_and(str::is_empty)
        {
            return Err(ConfigError::Message(
                "Production auth issuer, audience, JWT secret and JWKS path cannot be empty"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Validate the full security configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_production_auth()?;
        for origin in &self.cors_allowed_origins {
            if origin.trim().is_empty() {
                return Err(ConfigError::Message(
                    "CORS origin cannot be empty. Set HODEI_SECURITY__CORS_ALLOWED_ORIGINS or remove the entry".to_string(),
                ));
            }
            if origin.trim() == "*" {
                return Err(ConfigError::Message(
                    "CORS wildcard is not allowed. List explicit origins instead of '*'"
                        .to_string(),
                ));
            }
            if origin != &origin.to_ascii_lowercase() {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use lowercase scheme and host",
                    origin
                )));
            }
            if origin.contains('#') {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Origins cannot contain a fragment",
                    origin
                )));
            }
            let uri: axum::http::Uri = origin.parse().map_err(|_| {
                ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use a valid origin such as https://app.example",
                    origin
                ))
            })?;
            let default_port = match uri.scheme_str() {
                Some("http") => uri.port_u16() == Some(80),
                Some("https") => uri.port_u16() == Some(443),
                _ => false,
            };
            let non_canonical_port = uri.authority().is_some_and(|authority| {
                authority
                    .as_str()
                    .rsplit_once(':')
                    .is_some_and(|(_, port)| port.len() > 1 && port.starts_with('0'))
            });
            let valid_authority = uri
                .authority()
                .is_some_and(|authority| !authority.as_str().contains('@'))
                && uri.host().is_some_and(|host| !host.contains('*'))
                && matches!(uri.scheme_str(), Some("http" | "https"))
                && (uri.path().is_empty() || uri.path() == "/")
                && uri.query().is_none()
                && !default_port
                && !non_canonical_port;
            if !valid_authority {
                return Err(ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use scheme and authority only, such as https://app.example",
                    origin
                )));
            }
            HeaderValue::from_str(origin).map_err(|_| {
                ConfigError::Message(format!(
                    "Invalid CORS origin '{}'. Use a valid origin such as https://app.example",
                    origin
                ))
            })?;
        }
        Ok(())
    }
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
        build_config(environment_source())
    }

    /// Validate the entire configuration
    ///
    /// # Returns
    ///
    /// Ok(()) if configuration is valid, ConfigError with clear message if invalid
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.server.validate()?;
        self.security.validate()?;
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

/// The environment source that overrides the configuration.
///
/// The prefix separator has to be stated explicitly. Without it, config-rs
/// falls back to the *key* separator when one is given, so the prefix would
/// be looked for as `HODEI__` instead of `HODEI_`, and every
/// `HODEI_<SECTION>__<KEY>` variable would be **silently discarded**: the
/// documented override would be inert and the service would always boot with
/// the values from the TOML file.
///
/// `try_parsing` makes the collected values be parsed as numbers and booleans
/// instead of staying strings, which is what the typed configuration needs.
fn environment_source() -> Environment {
    Environment::with_prefix("HODEI")
        .prefix_separator("_")
        .separator("__")
        .try_parsing(true)
        .list_separator(",")
        .with_list_parse_key("security.cors_allowed_origins")
}

/// Builds the configuration from the file sources and the environment source.
///
/// The environment source is a parameter so that the precedence rules can be
/// exercised without mutating the process environment, which is shared by
/// every test running in the same binary.
fn build_config(environment: Environment) -> Result<AppConfig, ConfigError> {
    let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

    let s = Config::builder()
        // Default values (lowest precedence)
        .add_source(File::with_name("config/default").required(false))
        // Environment-specific configuration
        .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
        // Local overrides (higher precedence)
        .add_source(File::with_name("config/local").required(false))
        // Environment variables with HODEI_ prefix (highest precedence)
        .add_source(environment)
        .build()?;

    let app_config: AppConfig = s.try_deserialize()?;

    // Validate the configuration
    app_config.validate()?;

    Ok(app_config)
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

    /// The environment variables that must override the file configuration.
    ///
    /// The values are injected into the environment source instead of the
    /// process environment, so the test does not depend on the order in
    /// which the tests of this binary run.
    fn overrides() -> std::collections::HashMap<String, String> {
        [
            ("HODEI_SERVER__PORT", "41535"),
            ("HODEI_SERVER__HOST", "127.0.0.1"),
            ("HODEI_DATABASE__NAMESPACE", "hodei"),
            ("HODEI_DATABASE__DATABASE", "artifacts"),
            ("HODEI_ROCKSDB__PATH", "/tmp/hodei-config-override.rocksdb"),
            ("HODEI_SCHEMA__REGISTER_IAM_ON_STARTUP", "true"),
            ("HODEI_LOGGING__LEVEL", "warn"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// The documented environment override has to reach the configuration
    ///
    /// `HODEI_<SECTION>__<KEY>` is the documented way to configure the
    /// service per environment, and it is advertised by the startup error
    /// message. It used to be inert: config-rs looked for the prefix as
    /// `HODEI__` and discarded every variable, so the service always booted
    /// with the values from the TOML file.
    #[test]
    fn environment_variables_override_the_configuration() {
        let config = build_config(environment_source().source(Some(overrides())))
            .expect("the configuration must be loadable");

        assert_eq!(
            config.server.port, 41535,
            "HODEI_SERVER__PORT must override the configured port"
        );
        assert_eq!(
            config.server.host, "127.0.0.1",
            "HODEI_SERVER__HOST must override the configured host"
        );
        assert_eq!(
            config.rocksdb.path, "/tmp/hodei-config-override.rocksdb",
            "HODEI_ROCKSDB__PATH must override the configured database path"
        );
        assert_eq!(
            config.database.namespace.as_deref(),
            Some("hodei"),
            "HODEI_DATABASE__NAMESPACE must override the configured namespace"
        );
        assert!(
            config.schema.register_iam_on_startup,
            "HODEI_SCHEMA__REGISTER_IAM_ON_STARTUP must override the configured flag"
        );
        assert_eq!(
            config.logging.level, "warn",
            "HODEI_LOGGING__LEVEL must override the configured log level"
        );
    }

    /// An invalid environment override has to be rejected, not ignored
    ///
    /// An override that cannot be parsed must not be silently dropped: a
    /// service that boots with a configuration nobody asked for is worse
    /// than one that refuses to boot.
    #[test]
    fn an_invalid_environment_override_is_rejected() {
        let mut invalid = overrides();
        invalid.insert("HODEI_SERVER__PORT".to_string(), "not-a-port".to_string());

        let result = build_config(environment_source().source(Some(invalid)));

        assert!(
            result.is_err(),
            "an environment override that cannot be parsed must be rejected"
        );
    }

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
    fn security_defaults_are_closed() {
        let config = AppConfig::default();
        assert!(
            !config.security.admin_api_enabled,
            "the administrative surface must be closed by default"
        );
        assert!(
            config.security.cors_allowed_origins.is_empty(),
            "no browser origin is allowed by default"
        );
    }

    #[test]
    fn security_rejects_permissive_cors_configuration() {
        let mut config = AppConfig::default();
        config.security.cors_allowed_origins = vec!["*".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["not an origin".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example/".to_string()];
        assert!(config.validate().is_ok());

        config.security.cors_allowed_origins = vec!["https://user@app.example".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["HTTPS://app.example".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://*.example.com".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example:443".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example#fragment".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example:08443".to_string()];
        assert!(config.validate().is_err());

        config.security.cors_allowed_origins = vec!["https://app.example".to_string()];
        assert!(config.validate().is_ok());

        config.security.cors_allowed_origins = vec![
            "http://localhost:8080".to_string(),
            "https://[::1]:8443".to_string(),
            "https://app.example:8443".to_string(),
        ];
        assert!(config.validate().is_ok());
    }

    #[test]
    fn security_environment_overrides_are_honored() {
        let mut overrides = overrides();
        overrides.insert(
            "HODEI_SECURITY__ADMIN_API_ENABLED".to_string(),
            "true".to_string(),
        );
        overrides.insert(
            "HODEI_SECURITY__CORS_ALLOWED_ORIGINS".to_string(),
            "https://app.example,https://admin.example".to_string(),
        );

        let config = build_config(environment_source().source(Some(overrides)))
            .expect("security environment overrides must parse");

        assert!(config.security.admin_api_enabled);
        assert_eq!(
            config.security.cors_allowed_origins,
            vec![
                "https://app.example".to_string(),
                "https://admin.example".to_string()
            ]
        );
    }

    #[test]
    fn deployed_config_files_keep_the_admin_surface_closed() {
        for name in ["config/default", "config/development", "config/release"] {
            let config: AppConfig = Config::builder()
                .add_source(File::with_name(name))
                .build()
                .expect("config file must load")
                .try_deserialize()
                .expect("config file must deserialize");
            assert!(
                !config.security.admin_api_enabled,
                "{name} must keep the administrative surface closed"
            );
            assert!(
                config.security.cors_allowed_origins.is_empty(),
                "{name} must not allow browser origins by default"
            );
        }
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

        let invalid_config = RocksDbConfig {
            path: "".to_string(),
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());

        let invalid_config = RocksDbConfig {
            max_open_files: 0,
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());
    }
}
