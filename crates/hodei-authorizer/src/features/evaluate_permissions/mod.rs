//! Feature for evaluating authorization permissions with multi-layer security
//!
//! This feature provides comprehensive authorization evaluation by combining:
//! - IAM policies (user and group permissions)
//! - Service Control Policies (SCP) for organizational boundaries
//! - Cedar policy engine for evaluation
//! - Caching, logging, and metrics
//!
//! # Components
//!
//! - `dto`: Data Transfer Objects for authorization requests and responses
//! - `error`: Error types specific to authorization evaluation
//! - `ports`: Interfaces for cross-context dependencies (cache, logger, metrics, etc.)
//! - `use_case`: Core authorization evaluation logic
//! - `di`: Dependency injection container and factories
//! - `mocks`: Mock implementations for testing
//!
//! # Example
//!
//! ```no_run
//! use hodei_authorizer::features::evaluate_permissions::dto::{
//!     AuthorizationRequest, AuthorizationContext
//! };
//! use hodei_authorizer::features::evaluate_permissions::di::factories;
//! use hodei_authorizer::features::evaluate_permissions::EvaluatePermissionsUseCase;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create an authorization request
//! let request = AuthorizationRequest::new(
//!     "hrn:hodei:iam:us-east-1:default:user/alice".parse()?,
//!     "read".to_string(),
//!     "hrn:hodei:s3:us-east-1:default:bucket/my-bucket".parse()?,
//! );
//!
//! // Build the use case with dependencies
//! let use_case: EvaluatePermissionsUseCase<_, _, _> = factories::create_without_cache(
//!     // ... dependencies
//! );
//!
//! // Execute the authorization evaluation
//! let response = use_case.execute(request).await?;
//! # Ok(())
//! # }
//! ```

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Re-export main types for easier access
pub use dto::{
    AuthorizationContext, AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
    PolicyImpact,
};

pub use error::{EvaluatePermissionsError, EvaluatePermissionsResult};

pub use ports::{AuthorizationCache, AuthorizationLogger, AuthorizationMetrics};

pub use use_case::EvaluatePermissionsUseCase;

pub use di::{EvaluatePermissionsContainer, EvaluatePermissionsContainerBuilder, factories};

// Re-export mocks for testing
#[cfg(test)]
pub use mocks::{
    MockAuthorizationCache, MockAuthorizationLogger, MockAuthorizationMetrics, test_helpers,
};

/// Feature version and metadata
pub const FEATURE_VERSION: &str = "1.0.0";
pub const FEATURE_NAME: &str = "evaluate_permissions";

/// Configuration for the evaluate permissions feature
#[derive(Debug, Clone)]
pub struct EvaluatePermissionsConfig {
    /// Cache TTL in seconds (default: 300 = 5 minutes)
    pub cache_ttl_secs: u64,
    /// Enable/disable caching
    pub cache_enabled: bool,
    /// Enable/disable detailed logging
    pub detailed_logging: bool,
    /// Enable/disable metrics collection
    pub metrics_enabled: bool,
    /// Maximum evaluation time in milliseconds
    pub max_evaluation_time_ms: u64,
}

impl Default for EvaluatePermissionsConfig {
    fn default() -> Self {
        Self {
            cache_ttl_secs: 300,
            cache_enabled: true,
            detailed_logging: true,
            metrics_enabled: true,
            max_evaluation_time_ms: 5000, // 5 seconds
        }
    }
}

impl EvaluatePermissionsConfig {
    /// Create a new configuration with custom settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set cache TTL
    pub fn with_cache_ttl(mut self, ttl_secs: u64) -> Self {
        self.cache_ttl_secs = ttl_secs;
        self
    }

    /// Enable/disable cache
    pub fn with_cache_enabled(mut self, enabled: bool) -> Self {
        self.cache_enabled = enabled;
        self
    }

    /// Enable/disable detailed logging
    pub fn with_detailed_logging(mut self, enabled: bool) -> Self {
        self.detailed_logging = enabled;
        self
    }

    /// Enable/disable metrics
    pub fn with_metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    /// Set maximum evaluation time
    pub fn with_max_evaluation_time(mut self, time_ms: u64) -> Self {
        self.max_evaluation_time_ms = time_ms;
        self
    }
}

/// Utility functions for the evaluate permissions feature
pub mod utils {
    use super::*;
    use std::time::Duration;

    /// Convert configuration TTL to Duration
    pub fn ttl_to_duration(config: &EvaluatePermissionsConfig) -> Duration {
        Duration::from_secs(config.cache_ttl_secs)
    }

    /// Generate a cache key for authorization requests
    pub fn generate_cache_key(request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }

    /// Validate authorization request
    pub fn validate_request(
        request: &AuthorizationRequest,
    ) -> Result<(), EvaluatePermissionsError> {
        if request.action.is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Action cannot be empty".to_string(),
            ));
        }

        if request.principal.to_string().is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Principal cannot be empty".to_string(),
            ));
        }

        if request.resource.to_string().is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Resource cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Create a default authorization context if none provided
    pub fn ensure_context(request: &mut AuthorizationRequest) {
        if request.context.is_none() {
            request.context = Some(AuthorizationContext::default());
        }
    }
}

#[cfg(test)]
mod feature_tests {
    use super::mocks::test_helpers;
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_feature_version() {
        assert_eq!(FEATURE_VERSION, "1.0.0");
        assert_eq!(FEATURE_NAME, "evaluate_permissions");
    }

    #[test]
    fn test_config_default() {
        let config = EvaluatePermissionsConfig::default();
        assert_eq!(config.cache_ttl_secs, 300);
        assert!(config.cache_enabled);
        assert!(config.detailed_logging);
        assert!(config.metrics_enabled);
        assert_eq!(config.max_evaluation_time_ms, 5000);
    }

    #[test]
    fn test_config_builder() {
        let config = EvaluatePermissionsConfig::new()
            .with_cache_ttl(600)
            .with_cache_enabled(false)
            .with_detailed_logging(false)
            .with_metrics_enabled(false)
            .with_max_evaluation_time(10000);

        assert_eq!(config.cache_ttl_secs, 600);
        assert!(!config.cache_enabled);
        assert!(!config.detailed_logging);
        assert!(!config.metrics_enabled);
        assert_eq!(config.max_evaluation_time_ms, 10000);
    }

    #[test]
    fn test_utils_generate_cache_key() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
        let request = AuthorizationRequest::new(principal, "read".to_string(), resource);

        let cache_key = utils::generate_cache_key(&request);
        assert!(cache_key.contains("auth:"));
        assert!(cache_key.contains("read"));
    }

    #[test]
    fn test_utils_validate_request() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");

        // Valid request
        let valid_request =
            AuthorizationRequest::new(principal.clone(), "read".to_string(), resource.clone());
        assert!(utils::validate_request(&valid_request).is_ok());

        // Invalid request - empty action
        let invalid_request =
            AuthorizationRequest::new(principal.clone(), "".to_string(), resource.clone());
        assert!(utils::validate_request(&invalid_request).is_err());

        // Note: Hrn always produces a valid string representation,
        // so we skip the "empty principal" test as it's not realistic
        // with the current HRN implementation
    }

    #[test]
    fn test_utils_ensure_context() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
        let mut request = AuthorizationRequest::new(principal, "read".to_string(), resource);

        assert!(request.context.is_none());
        utils::ensure_context(&mut request);
        assert!(request.context.is_some());
    }

    #[test]
    fn test_utils_ttl_to_duration() {
        let config = EvaluatePermissionsConfig::new().with_cache_ttl(120);
        let duration = utils::ttl_to_duration(&config);
        assert_eq!(duration, Duration::from_secs(120));
    }
}
