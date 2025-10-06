//! Dependency Injection helpers for List Policies feature

use super::adapter::InMemoryPolicyLister;
use super::use_case::ListPoliciesUseCase;
use std::sync::Arc;

pub fn make_list_policies_uc() -> ListPoliciesUseCase<InMemoryPolicyLister> {
    let lister = Arc::new(InMemoryPolicyLister::new());
    ListPoliciesUseCase::new(lister)
}

pub fn make_list_policies_uc_with<L>(lister: Arc<L>) -> ListPoliciesUseCase<L>
where
    L: super::ports::PolicyLister,
{
    ListPoliciesUseCase::new(lister)
}

