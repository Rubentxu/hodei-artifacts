//! Dependency Injection helpers for Update Policy feature

use super::use_case::UpdatePolicyUseCase;
use std::sync::Arc;

/// Create an UpdatePolicyUseCase with custom validator and adapter
pub fn make_update_policy_uc_with<V: ?Sized, P: ?Sized>(
    validator: Arc<V>,
    adapter: Arc<P>,
) -> UpdatePolicyUseCase<V, P>
where
    V: super::ports::PolicyValidator,
    P: super::ports::UpdatePolicyPort,
{
    UpdatePolicyUseCase::new(validator, adapter)
}

#[cfg(test)]
mod tests {
    use super::*;
}
