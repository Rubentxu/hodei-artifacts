#![allow(dead_code)]

/*!
DI module for validate_policy feature.

This file is currently split into:
- legacy implementation (behind feature "legacy_infra")
- temporary stub (default) while refactor removes tight Cedar coupling.

Enable `--features legacy_infra` to recover the original behavior during the transition.
*/

#[cfg(feature = "legacy_infra")]
use super::use_case::ValidatePolicyUseCase;
#[cfg(feature = "legacy_infra")]
use crate::shared::application::{AuthorizationEngine, di_helpers};
#[cfg(feature = "legacy_infra")]
use anyhow::Result;
#[cfg(feature = "legacy_infra")]
use std::sync::Arc;

#[cfg(feature = "legacy_infra")]
/// Build ValidatePolicyUseCase wired with SurrealDB in-memory storage (legacy path)
pub async fn make_validate_policy_use_case_mem()
-> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) =
        di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ValidatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(all(feature = "legacy_infra", feature = "embedded"))]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Legacy embedded (RocksDB) builder.
    pub async fn make_validate_policy_use_case_embedded(
        path: &str,
    ) -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(
            path,
            di_helpers::test_helpers::test_entities_configurator,
        )
        .await?;
        let uc = ValidatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}

#[cfg(not(feature = "legacy_infra"))]
use super::use_case::ValidatePolicyUseCase;
#[cfg(not(feature = "legacy_infra"))]
use anyhow::Result;

#[cfg(not(feature = "legacy_infra"))]
/// Stub during refactor: AuthorizationEngine & storage path disabled.
/// Returns only the use case placeholder without backing engine.
/// Downstream callers should enable `legacy_infra` or migrate to new DI.
pub async fn make_validate_policy_use_case_mem() -> Result<(ValidatePolicyUseCase, ())> {
    Err(anyhow::anyhow!(
        "validate_policy DI disabled during refactor. Enable feature \"legacy_infra\" if you still need old engine wiring."
    ))
}

#[cfg(all(not(feature = "legacy_infra"), feature = "embedded"))]
pub mod embedded {
    // Stub embedded module while legacy infrastructure is disabled.
    // Intentionally left minimal.
}
