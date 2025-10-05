#![allow(dead_code)]

/*!
DI module for policy_playground feature.

This file provides:
- Legacy implementation (behind feature "legacy_infra") using old engine/storage wiring.
- A stub (default path) while refactor removes tight Cedar coupling.

Enable `--features legacy_infra` if you still need the previous behavior during migration.
*/

// -------------------- Legacy implementation (requires feature = "legacy_infra") --------------------
#[cfg(feature = "legacy_infra")]
use super::use_case::PolicyPlaygroundUseCase;
#[cfg(feature = "legacy_infra")]
use crate::shared::application::{AuthorizationEngine, di_helpers};
#[cfg(feature = "legacy_infra")]
use anyhow::Result;
#[cfg(feature = "legacy_infra")]
use std::sync::Arc;

#[cfg(feature = "legacy_infra")]
pub async fn make_policy_playground_use_case_mem()
-> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
    let (engine, _store) =
        di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = PolicyPlaygroundUseCase::new();
    Ok((uc, engine))
}

#[cfg(all(feature = "legacy_infra", feature = "embedded"))]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Legacy embedded builder (RocksDB path). Kept only while refactor is in progress.
    pub async fn make_policy_playground_use_case_embedded(
        path: &str,
    ) -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
        let (engine, _store) = di_helpers::build_engine_embedded(
            path,
            di_helpers::test_helpers::test_entities_configurator,
        )
        .await?;
        let uc = PolicyPlaygroundUseCase::new();
        Ok((uc, engine))
    }
}

// -------------------- Stub (default path without legacy_infra) --------------------
#[cfg(not(feature = "legacy_infra"))]
use anyhow::Result;

#[cfg(not(feature = "legacy_infra"))]
/// Stub during refactor: legacy engine wiring removed.
/// Returns an error instructing to enable `legacy_infra` if still needed.
pub async fn make_policy_playground_use_case_mem() -> Result<()> {
    Err(anyhow::anyhow!(
        "policy_playground DI disabled during refactor. Enable feature \"legacy_infra\" if you still need old engine wiring."
    ))
}

#[cfg(all(not(feature = "legacy_infra"), feature = "embedded"))]
pub mod embedded {
    // Stub module intentionally empty while legacy infra is disabled.
}
