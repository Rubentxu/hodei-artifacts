//! Feature: Evaluate Cedar Policies
//!
//! This feature provides a generic, domain-agnostic policy evaluation service
//! using Cedar's Authorizer. It can be used by any bounded context (IAM,
//! Organizations, etc.) to evaluate Cedar policies against authorization requests.
//!
//! # Architecture
//!
//! This feature follows Vertical Slice Architecture (VSA):
//! - `dto`: Data transfer objects for requests and responses
//! - `use_case`: Core business logic for policy evaluation
//!
//! # Usage
//!
//! ```rust,ignore
//! use policies::features::evaluate_policies::{
//!     EvaluatePoliciesUseCase,
//!     dto::{EvaluatePoliciesRequest, Decision},
//! };
//!
//! let use_case = EvaluatePoliciesUseCase::new();
//!
//! let request = EvaluatePoliciesRequest {
//!     policies: vec![
//!         r#"permit(principal, action == Action::"read", resource);"#.to_string()
//!     ],
//!     principal: "Iam::User::\"alice\"".to_string(),
//!     action: "Action::\"read\"".to_string(),
//!     resource: "S3::Bucket::\"my-bucket\"".to_string(),
//!     context: None,
//!     entities: vec![],
//! };
//!
//! let response = use_case.execute(request).await?;
//! assert_eq!(response.decision, Decision::Allow);
//! ```

pub mod dto;
pub mod use_case;

// Re-export main types for ergonomic use
pub use dto::{
    Decision, EntityDefinition, EvaluatePoliciesRequest, EvaluatePoliciesResponse,
    EvaluationDiagnostics,
};
pub use use_case::{EvaluatePoliciesError, EvaluatePoliciesUseCase};
