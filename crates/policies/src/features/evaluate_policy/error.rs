//! Feature-specific errors for policy evaluation

use shared::hrn::PolicyId;
use thiserror::Error;

/// Feature-specific error types for policy evaluation
#[derive(Debug, Error)]
pub enum EvaluatePolicyError {
    /// Policy not found
    #[error("Policy not found: {0}")]
    PolicyNotFound(PolicyId),

    /// Invalid policy syntax
    #[error("Invalid policy syntax: {0}")]
    InvalidPolicySyntax(String),

    /// Policy compilation error
    #[error("Policy compilation error: {0}")]
    PolicyCompilationError(String),

    /// Evaluation context error
    #[error("Evaluation context error: {0}")]
    EvaluationContextError(String),

    /// Cedar policy engine error
    #[error("Cedar policy engine error: {0}")]
    CedarEngineError(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Timeout error
    #[error("Evaluation timeout: {0}ms")]
    Timeout(u64),

    /// Resource quota exceeded
    #[error("Resource quota exceeded: {0}")]
    ResourceQuotaExceeded(String),

    /// Concurrent evaluation limit exceeded
    #[error("Concurrent evaluation limit exceeded: {0}")]
    ConcurrentLimitExceeded(u32),
}

impl EvaluatePolicyError {
    /// Convert to HTTP status code
    pub fn http_status(&self) -> u16 {
        match self {
            EvaluatePolicyError::PolicyNotFound(_) => 404,
            EvaluatePolicyError::InvalidPolicySyntax(_) => 400,
            EvaluatePolicyError::PolicyCompilationError(_) => 400,
            EvaluatePolicyError::EvaluationContextError(_) => 400,
            EvaluatePolicyError::ValidationError(_) => 400,
            EvaluatePolicyError::CedarEngineError(_) => 500,
            EvaluatePolicyError::DatabaseError(_) => 500,
            EvaluatePolicyError::SerializationError(_) => 500,
            EvaluatePolicyError::Timeout(_) => 504,
            EvaluatePolicyError::ResourceQuotaExceeded(_) => 429,
            EvaluatePolicyError::ConcurrentLimitExceeded(_) => 429,
        }
    }

    /// Error category for metrics
    pub fn error_category(&self) -> &'static str {
        match self {
            EvaluatePolicyError::PolicyNotFound(_) => "not_found",
            EvaluatePolicyError::InvalidPolicySyntax(_) => "validation",
            EvaluatePolicyError::PolicyCompilationError(_) => "compilation",
            EvaluatePolicyError::EvaluationContextError(_) => "validation",
            EvaluatePolicyError::ValidationError(_) => "validation",
            EvaluatePolicyError::CedarEngineError(_) => "engine",
            EvaluatePolicyError::DatabaseError(_) => "database",
            EvaluatePolicyError::SerializationError(_) => "serialization",
            EvaluatePolicyError::Timeout(_) => "timeout",
            EvaluatePolicyError::ResourceQuotaExceeded(_) => "quota",
            EvaluatePolicyError::ConcurrentLimitExceeded(_) => "concurrency",
        }
    }

    /// Whether this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            EvaluatePolicyError::DatabaseError(_)
                | EvaluatePolicyError::Timeout(_)
                | EvaluatePolicyError::ResourceQuotaExceeded(_)
                | EvaluatePolicyError::ConcurrentLimitExceeded(_)
        )
    }
}