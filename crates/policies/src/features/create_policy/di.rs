use std::sync::Arc;
use anyhow::Result;
use crate::shared::application::{AuthorizationEngine, di_helpers};
use super::use_case::CreatePolicyUseCase;

/// Build CreatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered. 
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_create_policy_use_case_mem() -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator).await?;
    let uc = CreatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build CreatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_create_policy_use_case_embedded(
        path: &str,
    ) -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::no_entities_configurator).await?;
        let uc = CreatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
