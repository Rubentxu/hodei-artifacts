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
    ports::{
        IamPolicyProvider as EvalIamPolicyProvider, OrganizationBoundaryProvider as EvalOrgProvider,
    },
    use_case::EvaluatePermissionsUseCase,
};
