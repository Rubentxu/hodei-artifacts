// Features module - only schema-related functionality remains
// Most features are gated behind "legacy_infra" during refactor

#[cfg(feature = "legacy_infra")]
pub mod batch_eval;

// New refactored feature - always available
pub mod create_policy;

#[cfg(feature = "legacy_infra")]
pub mod evaluate_policies;

#[cfg(feature = "legacy_infra")]
pub mod policy_analysis;

#[cfg(feature = "legacy_infra")]
pub mod policy_playground;

#[cfg(feature = "legacy_infra")]
pub mod policy_playground_traces;

// validate_policy available but with stub implementation when legacy_infra is disabled
pub mod validate_policy;
