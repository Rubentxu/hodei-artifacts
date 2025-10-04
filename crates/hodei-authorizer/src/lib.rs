pub mod application;
pub mod contracts;
pub mod dto;
pub mod features;

// Re-export IamPolicyProvider from hodei-iam for convenience
pub use hodei_iam::shared::application::ports::{
    IamPolicyProvider as EvalIamPolicyProvider, IamPolicyProviderError,
};

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
