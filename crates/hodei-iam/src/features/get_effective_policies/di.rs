use std::sync::Arc;
use super::ports::{UserFinderPort, GroupFinderPort, PolicyFinderPort};
use super::use_case::GetEffectivePoliciesUseCase;

/// Factory for creating GetEffectivePoliciesUseCase instances
///
/// This factory encapsulates the dependency injection logic for the
/// GetEffectivePoliciesUseCase, making it easier to construct instances with
/// different implementations of the ports.
pub struct GetEffectivePoliciesUseCaseFactory;

impl GetEffectivePoliciesUseCaseFactory {
    /// Build a GetEffectivePoliciesUseCase instance
    ///
    /// # Arguments
    /// * `user_finder` - Implementation of UserFinderPort for user lookup
    /// * `group_finder` - Implementation of GroupFinderPort for group lookup
    /// * `policy_finder` - Implementation of PolicyFinderPort for policy lookup
    ///
    /// # Returns
    /// * A new GetEffectivePoliciesUseCase instance
    pub fn build<UF, GF, PF>(
        user_finder: Arc<UF>,
        group_finder: Arc<GF>,
        policy_finder: Arc<PF>,
    ) -> GetEffectivePoliciesUseCase<UF, GF, PF>
    where
        UF: UserFinderPort,
        GF: GroupFinderPort,
        PF: PolicyFinderPort,
    {
        GetEffectivePoliciesUseCase::new(user_finder, group_finder, policy_finder)
    }
}
