// crates/iam/src/features/validate_policy/mod.rs

//! Policy validation feature module
//! 
//! This module implements comprehensive policy validation following VSA (Vertical Slice Architecture)
//! principles. It provides syntax, semantic, and HRN validation for Cedar policies.
//! 
//! ## Architecture
//! 
//! The module follows Clean Architecture with clear separation of concerns:
//! - **DTOs**: Data transfer objects for requests and responses
//! - **Ports**: Interfaces defining the contracts for external dependencies
//! - **Use Cases**: Business logic for policy validation
//! - **Adapters**: Implementations of ports using external services (Cedar)
//! - **API**: HTTP handlers for the validation endpoints
//! - **DI**: Dependency injection container for wiring components
//! 
//! ## Usage
//! 
//! ```rust
//! use crate::features::validate_policy::{ValidatePolicyContainer, ValidatePolicyRequest};
//! 
//! // Create the container with all dependencies
//! let container = ValidatePolicyContainer::new()?;
//! 
//! // Use the validation service
//! let request = ValidatePolicyRequest::new("permit(principal, action, resource);".to_string());
//! let response = container.validation_service().validate_policy(request).await?;
//! 
//! println!("Policy is valid: {}", response.is_valid);
//! ```

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Test modules
#[cfg(test)]
mod dto_test;
#[cfg(test)]
mod adapter_test;
#[cfg(test)]
mod use_case_test;
#[cfg(test)]
mod api_test;
#[cfg(test)]
mod performance_test;

// Re-export main types for convenience
pub use dto::{
    ValidatePolicyRequest, ValidatePolicyResponse, PolicyValidationDetails,
    ValidationOptions, ValidationMetrics, SyntaxValidationResult,
    SemanticValidationResult, HrnValidationResult, ValidationWarning,
    SyntaxError, SemanticError, HrnError, SemanticErrorType, WarningType,
};

pub use ports::{
    PolicyValidationService, PolicySyntaxValidator, PolicySemanticValidator,
    PolicyHrnValidator, ValidationMetricsCollector, ValidationConfigProvider,
    ValidationType, PerformanceThresholds, ValidationContext,
};

pub use use_case::ValidatePolicyUseCase;

pub use adapter::{
    CedarValidationAdapter, SimpleMetricsCollector, DefaultValidationConfigProvider,
};

pub use api::{
    ValidatePolicyApi, validate_policy_handler, health_check_handler,
    create_validation_router,
};

pub use di::{
    ValidatePolicyContainer, ValidatePolicyContainerBuilder,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::errors::IamError;

    #[tokio::test]
    async fn test_feature_integration() -> Result<(), IamError> {
        // Test that all components work together
        let container = ValidatePolicyContainer::new()?;
        
        let request = ValidatePolicyRequest::new(
            "permit(principal, action, resource);".to_string()
        );
        
        let response = container.validation_service().validate_policy(request).await?;
        
        // Should succeed for basic valid policy
        assert!(response.is_valid);
        assert!(response.validation_result.syntax.is_valid);
        assert!(response.validation_result.semantic.is_valid);
        assert!(response.validation_result.hrn.is_valid);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_feature_with_invalid_policy() -> Result<(), IamError> {
        let container = ValidatePolicyContainer::new()?;
        
        let request = ValidatePolicyRequest::new(
            "invalid policy syntax".to_string()
        );
        
        let response = container.validation_service().validate_policy(request).await?;
        
        // Should fail for invalid policy
        assert!(!response.is_valid);
        
        Ok(())
    }

    #[test]
    fn test_container_builder() {
        let container = ValidatePolicyContainerBuilder::new().build();
        assert!(container.is_ok());
    }

    #[test]
    fn test_router_creation() {
        let container = ValidatePolicyContainer::new().unwrap();
        let _router = container.create_router();
        // Should not panic
    }
}