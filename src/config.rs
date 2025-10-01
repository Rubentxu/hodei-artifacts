use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    pub cors: CorsConfig,
    pub metrics: MetricsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub shutdown_timeout_seconds: u64,
    pub request_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub connection_timeout_seconds: u64,
    pub retry_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    pub allow_origins: Vec<String>,
    pub allow_headers: Vec<String>,
    pub allow_methods: Vec<String>,
    pub max_age: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub prometheus_registry: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        Ok(Self {
            server: ServerConfig {
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "3000".to_string())
                    .parse()
                    .map_err(|_| ConfigError::InvalidPort)?,
                shutdown_timeout_seconds: env::var("SHUTDOWN_TIMEOUT")
                    .unwrap_or_else(|_| "30".to_string())
                    .parse()
                    .unwrap_or(30),
                request_timeout_seconds: env::var("REQUEST_TIMEOUT")
                    .unwrap_or_else(|_| "30".to_string())
                    .parse()
                    .unwrap_or(30),
            },
            database: DatabaseConfig {
                url: env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "baas_mvp_api.db".to_string()),
                max_connections: env::var("DB_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
                connection_timeout_seconds: env::var("DB_CONNECTION_TIMEOUT")
                    .unwrap_or_else(|_| "5".to_string())
                    .parse()
                    .unwrap_or(5),
                retry_attempts: env::var("DB_RETRY_ATTEMPTS")
                    .unwrap_or_else(|_| "3".to_string())
                    .parse()
                    .unwrap_or(3),
            },
            logging: LoggingConfig {
                level: env::var("LOG_LEVEL")
                    .unwrap_or_else(|_| "info,policy_baas_mvp=debug".to_string()),
                format: match env::var("LOG_FORMAT").as_deref() {
                    Ok("json") => LogFormat::Json,
                    Ok("compact") => LogFormat::Compact,
                    _ => LogFormat::Pretty,
                },
            },
            cors: CorsConfig {
                allow_origins: env::var("CORS_ORIGINS")
                    .unwrap_or_else(|_| "*".to_string())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                allow_headers: env::var("CORS_HEADERS")
                    .unwrap_or_else(|_| "content-type,authorization,x-request-id".to_string())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                allow_methods: env::var("CORS_METHODS")
                    .unwrap_or_else(|_| "GET,POST,PUT,DELETE,OPTIONS".to_string())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                max_age: env::var("CORS_MAX_AGE")
                    .ok()
                    .and_then(|s| s.parse().ok()),
            },
            metrics: MetricsConfig {
                enabled: env::var("METRICS_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                endpoint: env::var("METRICS_ENDPOINT")
                    .unwrap_or_else(|_| "/metrics".to_string()),
                prometheus_registry: env::var("PROMETHEUS_REGISTRY")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
            },
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid port configuration")]
    InvalidPort,
    #[error("Missing required environment variable: {0}")]
    MissingEnvVar(String),
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
}
