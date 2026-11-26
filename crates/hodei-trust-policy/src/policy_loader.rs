//! A [`PolicyLoader`] scoped to one account, over the durable lifecycle.
//!
//! The policy that decides for an account is the revision its head names.
//! The loader is fail-closed: a set that was never activated is an error
//! (never an empty policy that would let everything through), and a
//! revision whose content no longer hashes to its digest is an error too.

use std::sync::Arc;

use async_trait::async_trait;

use crate::policy::TrustPolicy;
use crate::ports::{DecisionStoreError, LifecycleStore, PolicyLoader};
use crate::registry::PolicyRegistry;

/// Loads the active policy of one account from a [`LifecycleStore`].
///
/// It is separate from the loader the promotion path already uses: that one
/// has no notion of account and is left untouched.
pub struct AccountPolicyLoader {
    store: Arc<dyn LifecycleStore>,
    account: String,
}

impl AccountPolicyLoader {
    /// A loader of `account`'s policies over `store`.
    pub fn new(store: Arc<dyn LifecycleStore>, account: impl Into<String>) -> Self {
        Self {
            store,
            account: account.into(),
        }
    }
}

#[async_trait]
impl PolicyLoader for AccountPolicyLoader {
    async fn active_policy(&self, policy_id: &str) -> Result<TrustPolicy, DecisionStoreError> {
        let head = self
            .store
            .head(&self.account, policy_id)
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?
            .ok_or_else(|| {
                DecisionStoreError::Unavailable(format!(
                    "the policy set {policy_id} has never been activated"
                ))
            })?;
        let revision = self
            .store
            .revision(&self.account, policy_id, head.sequence)
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?
            .ok_or_else(|| {
                DecisionStoreError::Unavailable(format!(
                    "the head of {policy_id} names revision {} that the store does not hold",
                    head.sequence
                ))
            })?;
        if !revision.digest_holds() || revision.digest != head.digest {
            return Err(DecisionStoreError::Unavailable(format!(
                "revision {} of {policy_id} does not match the digest of the head",
                head.sequence
            )));
        }
        Ok(revision.policy)
    }
}

/// Resolves the policy that decides **per account** from the durable head.
///
/// This is what makes activating or rolling back in the Policy Studio change
/// the policy the service actually applies: every decision reads the head of
/// the account it is made for, so there is no process-wide state to keep in
/// step and nothing to rebuild after a restart.
///
/// An account without a durable head fails closed
/// (`DecisionStoreError::Unavailable`), never an empty policy. The only
/// exception is an explicit, operator-declared seed (the deployment's trust
/// policy file): when present it answers for accounts that have never
/// activated anything, and it is never consulted for an account that has a
/// head, nor when the store itself fails.
pub struct AccountScopedPolicies {
    store: Arc<dyn LifecycleStore>,
    seed: Option<PolicyRegistry>,
}

impl AccountScopedPolicies {
    /// A resolver over `store` with no seed: no head means no policy.
    pub fn new(store: Arc<dyn LifecycleStore>) -> Self {
        Self { store, seed: None }
    }

    /// Declares the deployment seed that answers for accounts without a head.
    pub fn with_seed(mut self, seed: PolicyRegistry) -> Self {
        self.seed = Some(seed);
        self
    }
}

#[async_trait]
impl PolicyLoader for AccountScopedPolicies {
    /// Without an account there is no head to read: fail closed.
    async fn active_policy(&self, policy_id: &str) -> Result<TrustPolicy, DecisionStoreError> {
        Err(DecisionStoreError::Unavailable(format!(
            "the policy set {policy_id} is resolved per account, and no account was given"
        )))
    }

    async fn active_policy_for(
        &self,
        account: &str,
        policy_id: &str,
    ) -> Result<TrustPolicy, DecisionStoreError> {
        let head = self
            .store
            .head(account, policy_id)
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?;
        if head.is_none() {
            if let Some(policy) = self.seed.as_ref().and_then(|seed| seed.active(policy_id)) {
                return Ok(policy);
            }
        }
        AccountPolicyLoader::new(self.store.clone(), account)
            .active_policy(policy_id)
            .await
    }
}
