use std::sync::Arc;
use anyhow::Result;
use crate::shared::application::{AuthorizationEngine, di_helpers};
use super::use_case::PolicyPlaygroundUseCase;

/// Build PolicyPlaygroundUseCase (no storage required) and an AuthorizationEngine for consistency
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_policy_playground_use_case_mem() -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
    let (engine, _store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = PolicyPlaygroundUseCase::new();
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_policy_playground_use_case_embedded(
        path: &str,
    ) -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
        let (engine, _store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = PolicyPlaygroundUseCase::new();
        Ok((uc, engine))
    }
}
