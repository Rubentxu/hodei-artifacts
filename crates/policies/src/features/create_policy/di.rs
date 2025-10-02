use std::sync::Arc;

use anyhow::Result;

use crate::shared::application::{AuthorizationEngine, EngineBuilder};
use crate::shared::domain::principals;
use crate::shared::infrastructure::surreal::SurrealMemStorage;

use super::use_case::CreatePolicyUseCase;

/// Build CreatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
pub async fn make_use_case_mem() -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let storage = Arc::new(SurrealMemStorage::new("policies", "policies").await?);

    let (engine, store) = {
        let mut builder = EngineBuilder::new();
        builder
            .register_entity_type::<principals::User>()?
            .register_entity_type::<principals::Group>()?;
        builder.build(storage)?
    };

    let uc = CreatePolicyUseCase::new(Arc::new(store));
    Ok((uc, Arc::new(engine)))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

    /// Build CreatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    pub async fn make_use_case_embedded(
        path: &str,
    ) -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let storage = Arc::new(SurrealEmbeddedStorage::new("policies", "policies", path).await?);

        let (engine, store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()?
                .register_entity_type::<principals::Group>()?;
            builder.build(storage)?
        };

        let uc = CreatePolicyUseCase::new(Arc::new(store));
        Ok((uc, Arc::new(engine)))
    }
}
