use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Database connection error: {0}")]
    DatabaseConnection(String),
    #[error("Policy engine error: {0}")]
    PolicyEngine(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Authorization error: {0}")]
    Authorization(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Logging setup error: {0}")]
    LoggingSetup(String),
    #[error("Server bind error")]
    ServerBind(#[from] std::io::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type, error_message) = match &self {
            AppError::Configuration(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "CONFIGURATION_ERROR",
                "Internal configuration error",
            ),
            AppError::LoggingSetup(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "LOGGING_ERROR",
                "Logging setup failed",
            ),
            AppError::DatabaseConnection(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "DATABASE_ERROR",
                "Database service unavailable",
            ),
            AppError::PolicyEngine(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "POLICY_ENGINE_ERROR",
                "Policy engine error",
            ),
            AppError::ServerBind(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "SERVER_ERROR",
                "Server binding error",
            ),
            AppError::Authorization(_) => (
                StatusCode::UNAUTHORIZED,
                "AUTHORIZATION_ERROR",
                "Authorization failed",
            ),
            AppError::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "Request validation failed",
            ),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND", "Resource not found"),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Internal server error",
            ),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", "Bad request"),
        };

        let body = Json(json!({
            "error": {
                "type": error_type,
                "message": error_message,
                "details": self.to_string(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }
        }));

        // Log error with appropriate level
        match &self {
            AppError::Configuration(_)
            | AppError::LoggingSetup(_)
            | AppError::DatabaseConnection(_)
            | AppError::PolicyEngine(_)
            | AppError::ServerBind(_)
            | AppError::Internal(_) => {
                tracing::error!("Application error: {}", self);
            }
            AppError::Authorization(_) | AppError::NotFound(_) => {
                tracing::warn!("Client error: {}", self);
            }
            AppError::Validation(_) | AppError::BadRequest(_) => {
                tracing::debug!("Validation error: {}", self);
            }
        }

        (status, body).into_response()
    }
}

// Convenience conversion functions
impl From<crate::config::ConfigError> for AppError {
    fn from(err: crate::config::ConfigError) -> Self {
        AppError::Configuration(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
