use super::use_case::DeletePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build DeletePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
pub async fn make_delete_policy_use_case_mem() -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = DeletePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build DeletePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
    pub async fn make_delete_policy_use_case_embedded(
        path: &str,
    ) -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = DeletePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
