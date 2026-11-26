//! The store of the promotion decisions.
//!
//! A decision is written once and read many times. The store keeps it as
//! it was made -- the policy, the context, the inputs and the reasons --
//! so that the decision can be reconstructed later without asking the
//! system to recompute anything, which is the only way to know that what
//! was decided is what is read.
//!
//! The store does not offer a way to change a decision. A decision that
//! can be edited is not a decision.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use hodei_trust_policy::ports::{DecisionStoreError, PromotionTargets};
use hodei_trust_policy::promotion::PromotionTarget;

pub mod dgraph;
pub mod lifecycle;
pub mod reactions;

pub use dgraph::{
    ClaimedIntent, DgraphDecisionStore, DgraphIntentError, DgraphIntentStore, DgraphTrustError,
    DgraphTrustStore, GrantActivation,
};
pub use lifecycle::DgraphLifecycleStore;

/// The destinations of a promotion, over a fixed list.
///
/// A deployment that has no distribution configured says so, and the
/// promotion answers that the operation is not supported.
#[derive(Debug, Clone, Default)]
pub struct StaticPromotionTargets {
    targets: Vec<PromotionTarget>,
}

impl StaticPromotionTargets {
    /// The destinations that exist.
    pub fn new(targets: Vec<PromotionTarget>) -> Self {
        Self { targets }
    }
}

#[async_trait]
impl PromotionTargets for StaticPromotionTargets {
    async fn target_of(
        &self,
        account: &str,
        _subject: &str,
    ) -> Result<Option<PromotionTarget>, DecisionStoreError> {
        Ok(self
            .targets
            .iter()
            .find(|target| target.account == account)
            .cloned())
    }
}
