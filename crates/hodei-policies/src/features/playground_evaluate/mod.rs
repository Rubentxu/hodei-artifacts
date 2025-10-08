//! Playground Evaluate Feature
//!
//! This feature provides ad-hoc policy evaluation capabilities for testing
//! and experimentation in a playground environment.
//!
//! The playground allows users to:
//! - Evaluate Cedar policies without persisting them
//! - Test policies against inline or stored schemas
//! - Execute authorization requests with custom context
//! - Receive detailed diagnostics and error reporting
//!
//! # Architecture
//!
//! This feature follows Vertical Slice Architecture (VSA) with all necessary
//! components self-contained within this module:
//!
//! - `dto`: Data Transfer Objects (Commands, Queries, Results)
//! - `error`: Feature-specific error types
//! - `ports`: Port traits for dependency inversion (ISP-compliant)
//! - `use_case`: Core business logic
//! - `di`: Dependency injection factory
//! - `mocks`: Test mocks for unit testing
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use hodei_policies::features::playground_evaluate::{
//!     PlaygroundEvaluateCommand,
//!     PlaygroundAuthorizationRequest,
//!     PlaygroundEvaluateUseCaseFactory,
//! };
//! use kernel::Hrn;
//!
//! // Create authorization request
//! let request = PlaygroundAuthorizationRequest::new(
//!     Hrn::new("User", "alice")?,
//!     Hrn::new("Action", "read")?,
//!     Hrn::new("Resource", "document1")?,
//! );
//!
//! // Create evaluation command with inline schema and policies
//! let command = PlaygroundEvaluateCommand::new_with_inline_schema(
//!     schema_json,
//!     vec![policy_text],
//!     request,
//! );
//!
//! // Build use case with dependencies
//! let use_case = PlaygroundEvaluateUseCaseFactory::build(
//!     schema_loader,
//!     policy_validator,
//!     policy_evaluator,
//!     context_converter,
//! );
//!
//! // Execute evaluation
//! let result = use_case.execute(command).await?;
//!
//! println!("Decision: {}", result.decision);
//! println!("Determining policies: {}", result.determining_policies.len());
//! ```

pub mod adapters;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
pub mod mocks;

#[cfg(test)]
mod use_case_test;

// Re-export for convenience
pub use di::PlaygroundEvaluateUseCaseFactory;
pub use dto::{
    AttributeValue, Decision, DeterminingPolicy, EvaluationDiagnostics,
    PlaygroundAuthorizationRequest, PlaygroundEvaluateCommand, PlaygroundEvaluateResult,
    PolicyEffect,
};
pub use error::PlaygroundEvaluateError;
pub use ports::{ContextConverterPort, PolicyEvaluatorPort, PolicyValidatorPort, SchemaLoaderPort};
pub use use_case::{PlaygroundEvaluatePort, PlaygroundEvaluateUseCase};
