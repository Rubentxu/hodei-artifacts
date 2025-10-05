//! hodei-authorizer: Authorization service for Hodei platform
//!
//! This crate provides authorization services that combine IAM policies with
//! Service Control Policies (SCPs) from organizations to make comprehensive
//! authorization decisions using the Cedar policy engine.
//!
//! # Features
//!
//! - `evaluate_permissions`: Core authorization evaluation feature that combines
//!   IAM policies and organizational boundaries (SCPs) to determine if a principal
//!   is authorized to perform an action on a resource.
//!
//! # Architecture
//!
//! This crate follows a hexagonal architecture with Vertical Slice Architecture (VSA)
//! principles, similar to other Hodei crates. It depends on cross-context ports
//! defined in the shared kernel (`kernel` crate) to access IAM and organizational
//! policy information without creating tight coupling between bounded contexts.
//!
//! # Example
//!
//! ```ignore
//! use hodei_authorizer::features::evaluate_permissions::{
//!     di::EvaluatePermissionsContainerBuilder,
//!     dto::{AuthorizationRequest, AuthorizationResponse},
//! };
//! use kernel::Hrn;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Build the authorization container with required dependencies
//! let container = EvaluatePermissionsContainerBuilder::new()
//!     // ... configure dependencies
//!     .build()?;
//!
//! // Create the use case
//! let use_case = container.build_use_case();
//!
//! // Create an authorization request
//! let request = AuthorizationRequest::new(
//!     Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/alice")?,
//!     "read".to_string(),
//!     Hrn::from_string("hrn:hodei:s3:us-east-1:default:bucket/my-bucket")?,
//! );
//!
//! // Evaluate permissions
//! let response: AuthorizationResponse = use_case.execute(request).await?;
//! # Ok(())
//! # }
//! ```

pub mod application;
pub mod contracts;
pub mod dto;
pub mod features;

// Re-export evaluate permissions feature
pub use features::evaluate_permissions::{
    di::{EvaluatePermissionsContainer, EvaluatePermissionsContainerBuilder},
    dto::{
        AuthorizationDecision as EvalAuthDecision, AuthorizationRequest as EvalAuthRequest,
        AuthorizationResponse,
    },
    error::EvaluatePermissionsError,
    ports::OrganizationBoundaryProvider as EvalOrgProvider,
    use_case::EvaluatePermissionsUseCase,
};
