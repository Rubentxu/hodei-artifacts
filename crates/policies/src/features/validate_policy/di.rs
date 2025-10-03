use super::use_case::ValidatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build ValidatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_validate_policy_use_case_mem() -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ValidatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build ValidatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_validate_policy_use_case_embedded(
        path: &str,
    ) -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = ValidatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
