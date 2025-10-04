pub mod application;
pub mod contracts;
pub mod dto;
pub mod features;

#[cfg(test)]
pub mod tests;

// Re-export evaluate permissions feature
pub use features::evaluate_permissions::{
    di::{EvaluatePermissionsContainer, EvaluatePermissionsContainerBuilder},
    dto::{
        AuthorizationRequest as EvalAuthRequest, AuthorizationResponse,
        AuthorizationDecision as EvalAuthDecision,
    },
    error::EvaluatePermissionsError,
    ports::{
        IamPolicyProvider as EvalIamPolicyProvider,
        OrganizationBoundaryProvider as EvalOrgProvider,
    },
    use_case::EvaluatePermissionsUseCase,
};
