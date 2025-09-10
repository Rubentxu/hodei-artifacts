// Security Crate
pub mod application;
pub mod domain;
pub mod features;
pub mod infrastructure;

#[cfg(test)]
mod test_cedar_parsing;

// Re-export commonly used types
pub use domain::authorization::{
    AuthorizationDecision, Principal, Resource, Action, Context, AttributeValue
};
pub use application::ports::{AuthorizationService, AuthorizationRequest};
pub use infrastructure::errors::SecurityError;
pub use infrastructure::validation::{
    PolicyValidator, PolicyValidationResult, PolicyValidationError, 
    PolicyValidationWarning, ValidationErrorType, PolicyValidationService,
    HrnValidator, ComprehensiveCedarValidator, ComprehensiveValidationResult
};