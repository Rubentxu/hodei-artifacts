/// Centralized DI helpers to avoid code duplication across features
/// 
/// This module provides reusable functions for building engines and storage,
/// allowing features to focus on their specific use case construction.

use std::sync::Arc;
use anyhow::Result;
use crate::shared::application::{AuthorizationEngine, EngineBuilder, PolicyStore};
use crate::shared::domain::PolicyStorage;
use crate::shared::infrastructure::surreal::SurrealMemStorage;

#[cfg(feature = "embedded")]
use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses in-memory storage (default dev/test)
pub async fn build_engine_mem<F>(configurator: F) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealMemStorage::new("policies", "policies").await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses embedded storage (RocksDB)
#[cfg(feature = "embedded")]
pub async fn build_engine_embedded<F>(
    path: &str,
    configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealEmbeddedStorage::new("policies", "policies", path).await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// No-op configurator - creates an engine with NO entities registered (domain agnostic)
pub fn no_entities_configurator(builder: EngineBuilder) -> Result<EngineBuilder> {
    Ok(builder)
}
