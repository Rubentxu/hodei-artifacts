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
